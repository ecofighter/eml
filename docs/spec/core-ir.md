# Core IR

位置づけ: 規範。

型付き HIR から変換する Core IR と、その上のパス (translate と evidence passing)、LIR への変換を定める。2つの IR に共通する規則 (ブロックの列、構造の規則、値の表現、データの配置、構造体の形、位置の規則、末尾の位置の呼び出し、基本操作と実行時のモデル) と、LIR のパスと実行の意味は [LIR](lir.md) にある。

## 実行系の方針

- 実行系は、2つの層の IR と、LIR を実行する [VM](vm.md) で構成する。ネイティブのバックエンドは未実装で、Cranelift で JIT と AOT のコードを出す設計である ([ネイティブ化の設計](../future/native.md))
  - Core IR は意味の層である。前向きの辺だけを持つ基本ブロックの列で、すべての中間値に名前を付け、Repr、データの配置、エフェクトの命令を持つ。RC と box と末尾呼び出しは持たない。後の最適化パス (インライン化、jump threading など) も Core IR に置く
  - LIR は、EP の後の Core IR を写した層である。エフェクトの命令を持たず、RC、box、末尾呼び出しを明示する。VM は LIR をバイトコードに変換して実行し、後のネイティブ化も LIR を入力にする
- 層を分けるのは、Lean の LCNF と IR (λRC) と同じ構成である。2つの層は別の型を持ち、LIR にエフェクトの命令がないことと、Core IR に RC と box がないことを、verifier でなく型で保証する。重なる部品は `eml_lir` から借りる ([LIR](lir.md) の「LIR の命令」)
- ブロックの列にしたのは、バイトコード VM に写すことと、ネイティブ化 (Cranelift) の土台にすることを見込んだためである。ブロックの列は、バイトコードへは1回の線形なループで写り、Cranelift のブロック引数へは 1:1 で写る

## Core IR

### Core IR の命令

型付き HIR から変換する IR で、すべての中間値に名前を付ける。関数は基本ブロックの列で、ブロックは引数、文の列、終端を持つ。

| 種類 | 命令 |
|---|---|
| 文 | `let x = 右辺`、`unpack v L #t(f1, .., fn)` |
| 右辺 | 呼び出し (`call`、`apply`、`clause`、`par`、`perform`、`resume`、`handle`)、`closure`、`con`、`const`、`extern`、`prim`、`drop` |
| 終端 | `return a`、`jump bN(a1, .., an)`、`switch a L { #0 -> bN, #1(y1, .., yn) -> bN, _ -> bN }`、`switch a { 1 -> bN, _ -> bN }` |
| 手放す | `drop x` (参照を1つ手放す) |
| エフェクト (translate の出力だけ) | `perform op args`、`handle E(init, body) { 節 } return r`、`resume k(v, s)` (内部の命令)、`drop k`、呼び出しの `mask` |
| evidence passing の出力 | `prim` (基本操作)、`clause` (節の呼び出し) |

- `perform`、`handle`、`resume`、呼び出しの `mask` は translate だけが出し、evidence passing (EP) が基本操作と節の呼び出しに置き換える (下の「evidence passing」)。`clause` と、REPL の根の2つ (`root_get`、`root_set`) と `var` の節の空の配列 (`empty_mut_array`) を除く `prim` は EP だけが出す
- LIR との違いは、RC の命令 (`dup`、`decref`、`release`)、`box` と `unbox`、末尾呼び出し `tail` を持たないことと、エフェクトの命令と `mask` を持つことである。RC の命令などは LIR のパスだけが入れる
- `par` (`Call::Par`) は LIR と同じ命令で、テキストの形も同じである ([LIR](lir.md) の「LIR の命令」)。`call`、`apply`、`resume` と同じく `mask` を持てる (`mask [State] par (,) #0(f.1, g.2)`)。EP は `mask` のある `par` をほかの呼び出しと同じく包み、`mask` のない `par` はそのまま残す。子が起こせるのは `never` の操作だけで、親はそれを継続を作らずに起こし直すので、`par` は yield の地点にならない。最適化も、`par` を handler に触れない文として扱う (下の「handler への特殊化」)。その場で動く節の本体に `par` があれば、ほかの呼び出しと同じく under の形にする (下の「`perform` と節の3つの形」)
- 関数は、名前、内部の関数かどうかの印 (`internal`。[LIR](lir.md) の「関数の ABI」)、一様な位置の始まり (`split`。[LIR](lir.md) の「関数の ABI」)、yield しうるかの印 (`yields`。下の「`yields` の印」)、入口の関数の印 (`clause`。[LIR](lir.md) の「入口の関数の印」)、変数の表、`ret` と `ret_mark`、ブロックの列を持つ。プログラムは、関数、入口、文字列の表、配置の表、構造体の形の表、エフェクトの表、ファイルの表を持つ。どちらも LIR と同じ欄で、LIR への変換がそのまま写す (下の「Core IR から LIR への変換」)
- ブロックの列の意味 (アトム、`con`、`jump`、`switch`、`unpack`、フィールドの束縛、消費と読む) は、LIR と同じである ([LIR](lir.md) の「ブロックの列」)。RC の命令、`box`、`unbox`、`tail` の項目は Core IR に当たらない。エフェクトの命令の引数は、ほかの呼び出しの引数と同じく消費である
- Core IR は、[LIR](lir.md) の「構造の規則」のうち印のない部分に従う。R1〜R5、R6 の支配の半分、R7 と R8 のうち境界の検査でない部分である。下の「verifier」の引き継ぐ検査も受ける。境界の検査に従わないのは、translate と EP の出力が、box の挿入より前には境界に合わないためである
- 値の表現、データの配置、構造体の形、位置の規則、関数の ABI、末尾の位置の呼び出しの定義は、LIR と同じである ([LIR](lir.md))。この文書は、translate がそれをどう決めるかを書く (下の「型の Repr」、「構造体にする型」、「translate の配置」)

### 型の Repr

- 変数は、型から決まる値の表現 (Repr) と、Kind の印 (下の「Kind の印」) を持つ。型検査が書き出す型は矢印の線形性を持ち、translate はそれを instance の名前と印と debug ビルドの検査に使う (下の「変換の規則」と「食い違いがないことの検査」)。translate は型から Repr を1つの関数 (`type_repr`) で決める。Repr と互換の関係は [LIR](lir.md) の「値の表現」にある

  | 型 | Repr |
  |---|---|
  | `Int` | `int` |
  | `Float` | `float` |
  | `Unit` (要素のないタプル)、`Error` | `unit` |
  | 引数のないコンストラクタだけの data | `enum` |
  | 構造体にする型 (下の「構造体にする型」) | `struct(S)` |
  | `String`、`Bytes`、`FileIn`、構造体にしない要素のあるタプル、全コンストラクタがフィールドを持つ data | `obj` |
  | 引数のないコンストラクタとフィールドを持つコンストラクタが混ざる data | `tobj` |
  | 関数 (関数の値 `&f` は即値)、型変数 | `tobj` |

- 多相な位置 (総称的なフィールド、`apply` の結果) の束縛も、具体化した型から Repr を決める。値を受ける位置の Repr は [LIR](lir.md) の「位置の規則」が決め、変数の Repr と互換でなければ、box の挿入が変換を入れる
- 局所の変数とブロックの引数の Repr は、具体化した型の Repr である
- translate が決める関数のシグネチャは、具体化した型の Repr である。トップレベルの関数は instance の代入をかけた宣言の型から (下の「変換の規則」)、ラムダ、節、handle の本体は使う所の型から、`$op$`、`$con$`、`$externN` は参照の型から、`$cont$` と `$cont$state` は `k` の型から決める。`$cont$` と `$cont$state` の `k` の引数だけは `tobj` である。`split` 以降の一様な位置は、box の挿入が一様にする ([LIR](lir.md) の「関数の ABI」)。EP が作る関数のシグネチャは、下の「evidence passing」の各項目で決まる
- 箱の位置 (extern の行、操作の表、入口の関数 `entry$<名前>` の `ret`) は、構造体の型を `obj` にする。translate はこの読み方を `boxed_repr` で決める。`type_repr` との違いは構造体の型だけである

### 構造体にする型

`type_repr` は、具体化した型ごとに次の条件をすべて満たすとき、その型を構造体の Repr にする ([LIR](lir.md) の「構造体の形」)。

- コンストラクタが1つでフィールドを持つ `data` (レコードを含む) か、要素のあるタプルである
- `data` は、宣言が再帰の組に入らない。自分自身を、直接か、ほかの宣言のフィールドを通して含む型は外す。`Iter` は `Option (a, Iter e a)` を通して自分を含むので外れ、融合の規則は `Iter` に当たる。タプルは宣言を持たないので、この条件は当たらない。要素が再帰する型でも、その要素は `obj` の葉になるだけである
- 開いた後の葉の語の数が 6 (`MAX_STRUCT_WORDS`) 以下である。`unit` の葉は 0 語と数え、`unit` でない葉が1つ以上要る。上限は計測のプログラムから決めたもので、ABI から決めたものではない
- 葉に一様な位置の型変数 (`_`) と節の操作ごとの型変数 (`OpVar`) を含まない。入れ子のタプルや `data` の中の葉も数えるので、`(Int, (_, Int))` も構造体にしない。含むと、呼ぶ側の `Pair Float Int` と呼ばれる側の `Pair _ Int` で形が違い、形どうしの変換が要るためである
- 型スキームの型変数 (具体化していない型変数) は `tobj` の葉と数える
- `Lin` のフィールドを許す。ふるいの差の組 `(j, marks)` が `MutArray` を持つためである。構造体の値は `Lin` のまま移す
- row の型引数を持つ型を許す。Repr は row によらないためである

構造体は flatten まで1つの変数のまま扱い、translate では葉に開かない。開くと式と pattern の結果がすべて値の並びになり、boxing が葉からまとまりを組み直すための情報を、optimize と evidence passing の間ずっと持つことになるためである。

語の数か `unit` の条件だけで構造体にしない入れ子の型は、外側の型の `obj` の1語の葉になる。構造体にしない単一コンストラクタの型は、`obj` である。状態を渡す形の複製が返す組は、型を持たないので、語の数と `unit` の条件だけを当てる (下の「handler の複製」)。

- 構造体の型の `con` は `struct(S)` の変数を束縛する。引数は形のフィールドの Repr と比べる正確な位置で、配置の宣言した Repr とは比べない
- 構造体の値を分けるのは、いつも `unpack` である。束縛するフィールドの Repr と印は、形のものになる。構造体の変数はタグの `switch` の scrutinee にならない
- `with_arg` の extern (`Std.FileIn`、`Std.Stdin`、`Std.Proc` のそれぞれ5つの読み、`Std.MutArray.read`、`length`、`Std.Async.task_cell`、`Std.IoTest.peer`、`op_stream`、`op_timed_out`) が作る組も、条件を満たせば構造体になる (下の「組を返す extern」)
- 入口の関数 `entry$<名前>` の `ret` は値1つにする。入口の値の型が構造体なら `ret` を `obj` にし、box の挿入が箱に入れる。VM の `Finish` は値を1つだけ受けるためである ([VM](vm.md) の「誤り、止めること、終わり」)
- 型から決める構造体 (`StructTypes`) と、型を持たない組 (`with_arg` の組と、`optimize` の状態を渡す形の複製が返す組) は、同じ語の数と `unit` の条件 (`MAX_STRUCT_WORDS`) で決める。型を持たない組の形は `tuple_struct` が作る

### Kind の印

変数と関数の戻り値 (`ret_mark`) は、Repr と並べて Kind の印 `unr` / `lin` / `poly` を持つ。意味と、印を使う検査 (V1、V2、作る所の検査) は [LIR](lir.md) の「Kind の印」と「Kind の印の検査」にある。印は Repr と同じく Core IR で決まり、後のパスは変えない。この節は、translate と EP と box の挿入が印をどう決めるかを書く。

#### translate が決める印

- 型のある所は `mark(ty) = TypeStore::linearity(ty)` で決める。型は instance の代入をかけた後の型なので、宣言に対して相対的な型 (`Rigid` か `Var` に値が依存する型) は現れない。`Flexible(k)` は `k` を返す。一様な位置と entry の鍵の `Flexible` は `uniform_kind`、生成器の鍵は `Poly`、推論が決めなかった型変数はその最小値である。写さない本体の `OpVar(_, k)` も `k` を返す
- Repr が RC の対象でない値 (`int`、`float`、`enum`、`unit`) の印は、型の Kind によらず `unr` にする。`Lin` と宣言したフィールドのない型 (`data Token : Lin = | Token`) の値も、Repr は `enum` で印は `unr` である。印は参照の RC と `lin` ビットを守るためのもので、スカラーとその箱は資源を持たないためである。値を1回だけ使うことは型検査が守る。`lin` にすると、box の挿入が足す `unr` の箱が `lin` の受け手に渡り (V2)、`yield` の遅い道で死ぬ箱を Perceus が `decref` する (V1)。値を包む `Option Token` や、`Token` を捕まえたクロージャは、型のとおり `lin` になる。下の2つの読み方 (`linearity` と `linearity_or_poly`) のどちらにも、この規則をかける
- 型が式から直接は読めない所は、次の規則で決める

  | 所 | 印 |
  |---|---|
  | handle の本体、操作の節、`return` の節のクロージャ | 捕まえた値の印の結び |
  | 出現から作る `con` (`materialize_once`。決定木の葉、`return` と `jump` で抜ける出口、`Bool` の文脈、知らない出口の合流で使う) | 検査した式 (タプルの式と構成子の適用) の型の印 |
  | 射影で取り出さない残りのフィールド | `BodyTypes::discarded` の型の印 |
  | 更新のフィールド | `Update::fields` の型の印 |
  | 分解した引数の合流のラベル | その引数の印 |
  | 文字列の定数、`if` の条件、`drop` の結果 | `unr` |

  - `con` を作る出現の印を、引数の印の結びにはしない。`Option (List FileIn)` の `Some []` のように引数が定数だけの出現は `unr` のままで、`unr` の値は `lin` の受け手に渡せる ([LIR](lir.md) の「Kind の印の検査」の V2)。出現が開いて作る `con` は、`Tag` なら `unr`、`con` で作った変数 (`Made`) ならその変数の印を運ぶ
  - 操作の節の `k` の印は、`k` の型の矢印の線形性から決める。操作の箇所の継続の多重度ではない
- 構造体の型の値の印も、ほかの型と同じく型の Kind から決める。RC の対象でない値の印は `unr` とする規則は、箱の位置の Repr (`boxed_repr`) で RC の対象かを見る。構造体の箱は `obj` なので、構造体は型の Kind に従う
- 宣言の型を代入せずに読む所 (操作の表) は、相対的な型も読める読み方 (`TypeStore::linearity_or_poly`) で決め、`Rigid` と `Var` に依存する値を `poly` にする。保守的なので、`(a, FileIn)` は `lin` でなく `poly` になる。`$op$` と `$con$` は参照の型ごとの instance なので、型のある所の規則で決める
- `k` を関数の値として包む `$cont$` と `$cont$state` は、引数 `k` を `poly` にする。生の継続の線形性 (`once` の操作なら `Lin`、`multi` なら `Unr`) は `k` の値の型から決まらないためである。再開に渡す値 `v` と状態 `s` と結果 `t` は、`k` の型から決める
- 導出とタプルの生成器 (`==`、`compare`、`show_prec`、`show`、`tag$` など) の変数はすべて `unr` にする。`Unr` のクラスの規則 ([型と Kind](types.md) の「`Unr` のクラス」) で、instance の頭の型変数は `Unr` だからである。生成器の鍵の `Flexible(Poly)` から決めると、`tag$` の `decref x` を V1 が拒む
- 表示関数 (`display$N$k` と `display$N$k$rest`) の変数もすべて `unr` にする。表示する値はいつも `Unr` である (下の「表示関数」)
- `materialize_once` は、同じ形の出現を1つの `con` にまとめるのを、`unr` の出現に限る。`unr` でない値を2か所で使うと、Perceus が `dup` するためである (`let (a, b) = (Some [], Some [])` を `Option (List FileIn)` で書く形)。`unr` でない出現は、作った `con` の表に入れない。入れると、同じ形の `unr` の出現が `lin` の `con` を使い回し、その `con` に `dup` が要るためである
- 決定木は、`con` で作った変数 `v` の引数を直接読むことがある。`v` の印が `unr` でないときは、次の2つがどちらも成り立つときにだけ開く。どちらかが成り立たなければ、`v` そのものを `switch` か `unpack` で分解する
  - `v` を検査する式が、`v` の唯一の使用である。`v` を束縛した局所変数が、本体の中でちょうど1回だけ参照されている (`let o = Some xs; ..; match o with ..` の `o`)。局所変数に束縛しない `con` の変数は、作った式の位置でだけ使われるので、これを満たす。開くとは、その `con` がどこでも死ぬということで、ほかの制御の道が `v` を使うと、Perceus が `lin` の引数を `dup` し、`lin` の `con` を `decref` するためである。`let v = Some xs; if c then (match v with ..) else close_opt v` がこれに当たる
  - 決定がすべて静的に決まるか (葉は1つで、その葉が `v` を受け取るか、部分を束縛して `v` が死ぬかのどちらかである)、どの枝も `v` か `v` を含む値を受け取らない。後者は、決定を作る前に、すべての枝のパターンを `v` の位置に照らして調べる。`v` の位置かその祖先の位置を変数のパターンで受ける枝と、`v` を含む別名を渡す枝があれば開かない。到達できるかは見ず、保守的に決める。部分行列でなくすべての枝を見るのは、`match (p, n) with | (x, 0) -> take x | ((a, []), _) -> ..` のように、`p` を開く部分行列に `p` を受け取る枝が入らないことがあるためである

  この規則では、線形のレコードやリストをその場で作って分解する形 (`let j = Job { .. }; let Job { log = Log { file, .. } } = j`) は、縮約の後に `con` を残さない。translate は `con` を出すが、開いた `con` は使われないので、縮約が消す ([LIR](lir.md) の「縮約」)。Core IR は値の意味で書き、所有を持たないので、葉も読む `lin` の引数を持つ使われない `con` は、Core IR として正しい。式の位置のタプルの式と構成子の適用 (まだ作っていない値) は、引数を直接読む。開かない変数は `v` そのものを分解するので、`con` と `unpack` と `release` が1組ずつ余分に要る

#### 決定木の葉の `drop`

`unr` でない値を分解し、そこから `unr` でない変数を何も束縛しない葉は、translate が `drop v` を出す。Perceus の `decref` は `unr` でない変数に置けないためである。

- 根: 検査した式の出現から、決定が開いた頭を知っている値をたどってたどり着く変数を根と呼ぶ。開くのは、式の位置のタプルの式と構成子の適用、頭を知っている値で抜ける出口、決定が開いた `con` の変数と、それらの入れ子である。決定が開かなかった `con` の変数は、その変数自体を根にする
- 条件: 根 `v` ごとに、次のすべてが成り立つとき、決定木の葉で `drop v` を出す
  - `v` の印が `unr` でなく、Repr が RC の対象である
  - 枝が、`v` から取り出した変数 (`v` の分解で得たフィールドと、その分解で得たもの) のうち、印が `unr` でないものを1つも束縛しない
  - 枝が束縛する出現と、渡す別名のどれも、`v` を含まない。`v` 自身も、`v` を中に持つ出現 (`match (Some h, n) with | (o, 0) -> ..` の `o` が受け取る `Some h`) も受け取らない
- 正しい理由: 枝が束縛する `unr` でない変数はちょうど1回使われ、消費されれば Perceus が `v` を `release` し、さらに分解されればその変数に同じ規則が当たる。それ以外の枝は、`v` の下にあるものを `unr` の値としてしか使わないので、`v` を drop してよい。型検査は `match` を `v` の使用と数えているので、drop は正直である。`v` が借りたフィールドなら、消費によって Perceus は持ち主を `release` する
- 枝が `v` から `unr` の RC の値 (`String` など) を束縛し、`drop v` の後で使うときは、Perceus はそのフィールドの所有を `dup` で取る (持ち主 `v` が `drop` に要るため)。`release` で取る形より RC の増減が1回ずつ増える。drop をフィールドの最後の使用の後に置けば増えないが、translate に生存の計算が要るので採らない

#### 戻らない呼び出しの前の `drop`

戻らない呼び出し ([線形性](linearity.md) の「戻らない呼び出し」) の前では、型検査が記録した変数 (`BodyTypes::implicit_drops`) を translate が `drop` で手放す。

- 置く所: 呼ばれる式と引数をすべて評価した後、最初の呼び出しの命令の前である。直接の呼び出し、`apply`、`perform`、extern の呼び出しのどれでも同じで、引数が余る呼び出しでは、引数のそろった呼び出しの前になる。引数の評価の間はまだ持っているので、持ち越しのパスも同じ範囲で持つものとして数える ([線形性](linearity.md) の「線形性の検査パス」)
- 手放す値: 記録した変数のうち印が `unr` でないものを、変数の番号の順に `drop v` で手放す。`unr` の変数は、生きていない所で Perceus が `decref` する。ラムダ、節、持ち上げたローカル関数の本体の中では、捕まえた変数は引数の変数に写っているので、その変数を手放す。記録した変数が持ち上げたローカル関数なら、関数は値を持たないので、その関数の持ち上げた捕獲 (下の「ローカル関数の持ち上げ」) を手放す
- 参照の数: 決定木が `con` の変数を開くかを決める参照の数 (上の「translate が決める印」) に、記録した `drop` も1回の参照として数える。一方の枝で `match` し、戻らない枝で手放す局所変数は2回参照されたことになり、決定木はその `con` を開かずに分解する。開くと、`drop` が持つ `con` と、決定木が直接読んだ `lin` の引数を、二重に所有することになるためである
- 呼び出しの前に評価して一時に持っている値 (`(f, fail "x")` の `f`) と、呼び出しをまたいで生きている値は手放さない。中断の巻き戻しが解放する ([VM](vm.md) の「巻き戻し」)

#### EP が足す変数の印

| 変数 | 印 | 理由 |
|---|---|---|
| evidence、`evv`、`ev_clause` が返す入口のクロージャ、`unit` と `int` の一時変数 (`prompt$` の `s`、つまり `yield_status` の `int` を含む) | `unr` | Perceus か基本操作が `dup` する、実行系の中で共有する入れ物である。操作の節は `Unr` の値しか捕まえない ([エフェクトと handler](effects.md) の「handler の意味」) |
| 状態 (`ev_take_state`)、κ、`closure resume$`・`closure rewind$`・`closure mask$[..]` で作る継続、handle の箇所の `x = apply body(())`、補助の関数 (`id$`、`compose$`、`prompt$`、`resume$`、`rewind$`、`mask$[..]`) の `ret`、`x`、`k`、`r`、`v` (値) と `resume$` の `s` (状態) と戻り値 | `poly` | `FileIn` を持ちうる。in-place の入口の状態は、translate がその状態の変数に付けた印を残す (`poly` より正確で、渡せる) |
| ダミー (`prim dummy()`) | `poly` | 戻り値の印が `lin` の関数からも返す |
| EP が作るクロージャ (`f$kN`、作り直す `compose$`、`yielding_entry` の `g`) | 捕まえた値の印の結び | `FileIn` を持つクロージャが `lin` になり、ヘッダの `lin` ビットが正しく立つ |
| `f$kN` と `f$jJ` の引数 | 元の関数の変数の表から写す | 同じ値である |
| `f$kN` と `f$jJ` を呼んだ結果 | 元の関数の戻り値の印 | |
| 同じ場で動く節の入口の戻り値 | 操作の表の戻り値の印 | Repr の規則と同じ形である |
| `yielding_entry` の戻り値 | 操作の表の戻り値の印。`never` の操作は戻り値を持たないので `poly` | ダミーしか返さない |

box の挿入が足す変数の印は、[LIR](lir.md) の「box の挿入」にある。

#### EP の明示の `drop`

Perceus は、`unr` でない変数に `decref` を置く所がある。V1 とぶつかるので、EP が明示の `drop` を出し、値を普通の消費にする。`drop` は消費なので、Perceus はそこに何も足さない。

| 所 | 実行時の値 | 出す `drop` |
|---|---|---|
| `prompt$` の yield と中断の経路 (状態 1、2、3) の `x` | いつもダミー (状態 2 では、巻き戻しが着地で書いたダミー) | 各経路の先頭で `drop x` |
| `prompt$` の中断の経路 (状態 2) の `ret` | `return` の節のクロージャ。捕まえた `FileIn` を持ちうる | 中断の経路の先頭で `drop ret` |
| `compose$` と `mask$[..]` の遅い道の `r` | いつもダミー | 遅い道の先頭で `drop r` |
| ユーザーの関数の yield の地点の結果 (遅い道) | いつもダミー | 遅い道の先頭で `drop x` |

- どれも、結果の Repr が RC の対象 (`obj` か `tobj`) で、印が `unr` でないときにだけ出す
- 中断の経路の `drop ret` は、EP が足す drop のうち、唯一の本当の後始末である。中断で `return` の節のクロージャを手放し、捕まえていた `FileIn` を閉じる。中断のときの drop は [線形性](linearity.md) の「暗黙の後始末」が許している。中断で外れる途中のフレームの生きている値は、EP が drop を置かず、VM の巻き戻しが表で手放す ([VM](vm.md) の「巻き戻し」)。新しく足す drop は、死んだ yield の結果と `ret` だけである
- `drop` の意味は「参照を1つ手放す」である。`poly` の値は実は `Unr` で共有されていることがある (多重の継続を2回再開して写した `resume$` のクロージャが捕まえる `ret` など) ので、「すぐに破棄する」の意味にはしない。検査モードでは、ヘッダの `lin` ビットの立った物体だけが、共有の上の drop を誤りにする
- translate が出す決定木の葉の `drop` と戻らない呼び出しの前の `drop` (上の「決定木の葉の `drop`」と「戻らない呼び出しの前の `drop`」) と合わせて、V1 の下で `unr` でない変数を手放す命令は、`drop` と分解のときの `release` だけになる

#### 表の印の列

- extern の行 (`eml_extern::FunctionRow`) は、`params` と `ret` と並べて `param_marks: &'static [Mark]` と `ret_mark: Mark` を持つ。印は LIR の印と同じ3値 (`unr`、`lin`、`poly`) で、`Mark` は `eml_extern` が持ち `eml_lir` が再公開する。extern の関数の引数に現れる型変数はスキームで `Unr` に固定される ([宣言](declarations.md) の「`extern`」) ので、型変数の位置 (`Std.Array.get` の要素) と、Kind が要素に従う `Array a` は `unr` である。固定しない位置 (コールバックの最後の結果、結果だけの型変数、預かる引数の型変数) は `poly` で、verifier はどの印の値も通す。`exit` の結果の印は、行の `ret_mark` でなく、translate が具体化した結果の型から決める ([LIR](lir.md) の「戻らない extern」)

  | 行 | 引数 | 結果 |
  |---|---|---|
  | `Std.FileIn.open` | `Unr` | `Lin` (`FileIn`) |
  | `Std.FileIn.read_all` | `Lin` | `Unr` (`String`) |
  | `Std.FileIn.read_line_raw` | `Lin` | `Unr` (`String`) |
  | `Std.FileIn.read_all_bytes`、`read_line_bytes_raw` | `Lin` | `Unr` (`Bytes`) |
  | `Std.FileIn.read_chunk` | `Unr`、`Lin` | `Unr` (`Bytes`) |
  | `Std.FileIn.close` | `Lin` | `Unr` |
  | `Std.FileOut.create`、`append` | `Unr` | `Lin` (`FileOut`) |
  | `Std.FileOut.write`、`write_bytes`、`write_line` | `Unr`、`Lin` | `Lin` (同じハンドル) |
  | `Std.FileOut.flush` | `Lin` | `Lin` |
  | `Std.FileOut.close_raw` | `Lin` | `Unr` |
  | `Std.Stdin.open` | `Unr` | `Lin` (`Stdin`) |
  | `Std.Stdin.read_all`、`read_line_raw` | `Lin` | `Unr` (`String`) |
  | `Std.Stdin.read_all_bytes`、`read_line_bytes_raw` | `Lin` | `Unr` (`Bytes`) |
  | `Std.Stdin.read_chunk` | `Unr`、`Lin` | `Unr` (`Bytes`) |
  | `Std.Stdin.close` | `Lin` | `Unr` |
  | `Std.Stdout.open`、`Std.Stderr.open` | `Unr` | `Lin` (`Stdout`、`Stderr`) |
  | `Std.Stdout` と `Std.Stderr` の `write`、`write_bytes`、`write_line` | `Unr`、`Lin` | `Lin` (同じハンドル) |
  | `Std.Stdout.flush` | `Lin` | `Lin` |
  | `Std.Stdout.close`、`Std.Stderr.close` | `Lin` | `Unr` |
  | `Std.Array.thaw` | `Unr` | `Lin` (`MutArray a`) |
  | `Std.MutArray.make` | `Unr`、`Unr` | `Lin` |
  | `Std.MutArray.empty` | `Unr` | `Lin` |
  | `Std.MutArray.length` | `Lin` | `Unr` (`Int`) |
  | `Std.MutArray.read` | `Lin`、`Unr` | `Unr` (要素) |
  | `Std.MutArray.write` | `Lin`、`Unr`、`Unr` | `Lin` |
  | `Std.MutArray.freeze` | `Lin` | `Unr` (`Array a`) |
  | `Std.Par.par` | `poly`、`poly` | `poly` |
  | `Std.Async.start_raw`、`run_raw` | `poly` | `Lin` (`Task a`) |
  | `Std.Async.park`、`park_time`、`park_drain`、`park_send`、`park_recv` | `Unr`、`Lin` | `Unr` |
  | `Std.Async.ready` | `Lin` | `Unr` |
  | `Std.Async.finish_ok` | `poly` | `Unr` |
  | `Std.Async.task_cell` | `Lin` | `Unr` (`Cell`) |
  | `Std.Async.take_result`、`run_result` | `Lin` | `poly` |
  | `Std.Async.detach`、`cancel_raw` | `Lin` | `Unr` |
  | `Std.Async.start_in_raw` | `Unr`、`poly` | `Lin` (`Task a`) |
  | `Std.Chan.push_raw` | `poly`、`Unr` | `Unr` |
  | `Std.Chan.pop_raw` | `Unr` | `poly` |
  | ほかのすべての行 | すべて `Unr` | `Unr` |

  `with_arg` の行 (`Std.FileIn`、`Std.Stdin`、`Std.Proc` のそれぞれ5つの読み、`Std.MutArray.length`、`read`、`Std.Async.task_cell`、`Std.IoTest.peer`、`op_stream`、`op_timed_out`) の結果の列は、組の最初の要素の Kind である (下の「組を返す extern」)。`std/` のシグネチャとの結び付けは、Repr と同じテスト (`crates/eml_core_ir/tests/externs.rs`) で、`TypeStore::linearity_or_poly` を使って確かめる。型変数に依存する宣言の型 (`Array a`、`a`) は `Poly` と読めるが、固定される位置の行は `unr` で、固定しない位置の行は `poly` なので、`Poly` はどちらの行とも合うものとして比べる。`MutArray a` は型変数によらず `Lin` と読める。戻らない extern の結果は、テストも行と比べない ([LIR](lir.md) の「戻らない extern」)
- 基本操作の表の印の列は [LIR](lir.md) の「基本操作」にある。EP が自分の変数に付ける印と同じなので、`prim` の V2 が EP 自身を確かめる
- 操作の表 (`OperationInfo`) は、Repr の `params` と `ret` と並べて、引数と戻り値の印の列 (`param_marks`、`ret_mark`) を持つ。操作の宣言の型を、相対的な型も読める読み方で読み、`Rigid` と `Var` に依存する値を `poly` にする。呼ぶ側から節へ渡る位置の矢印は宣言で `Unr` に固定されている ([エフェクトと handler](effects.md) の「handler の意味」) ので `unr` になる。V2 は `clause` の呼び出しと、`clause` の印の付いた関数の引数と戻り値を、この列と比べる。テキストの形では、効果の宣言が列を書く ([テスト戦略](../implementation/testing.md) の「LIR のテキストの形」)

### translate の配置

- 配置のコンストラクタの、宣言したフィールドの Repr は、コンストラクタの型スキームのフィールドの型から決める。型変数のフィールドは `tobj` で、具体化した型は見ない。構造体にする型のフィールドも `tobj` である (`layout_field_repr`)。型変数のフィールドと同じ一様な位置で、互換の位置として box の挿入が箱との間を変換する。`data Shape = | Ball Vec3 Float | Empty` の `Ball` は `(tobj, float)` である。`data Pair a = Pair a Int` は `Pair(tobj, int)` になり、`Option Int` と `Option a` は同じ配置 `Option { None, Some(tobj) }` を使う。表の形は [LIR](lir.md) の「データの配置」にある
- translate の型の Repr と配置の Repr は、同じ関数 (`data_repr`) で決める。構造体にする型も配置を持ち、配置の Repr は `obj` である。構造体の箱は、この配置の `con` と同じ形の物体である ([LIR](lir.md) の「構造体の形」)
- 配置を持つのは、translate が出す IR が指すデータ型 (Prelude の `Bool` を含む) と、組の大きさごとの配置である。extern の型と `Unit` は持たない。名前は関数と同じく、入口以外のモジュールの型を修飾する (`Prelude.Bool`)。組の配置は宣言がないので大きさだけで決まり、名前は `(,)`、`(,,)` のように、大きさより1少ないカンマを括弧で囲む。組のコンストラクタは1つで、名前は配置と同じで、フィールドはどれも `tobj` である
- translate は、命令を出すそれぞれの所で、コンストラクタか型から配置を決める。値の型からは決めない。配置は最初に使った順に表に入れる。translate の後のパス (EP と LIR のパス) は表を変えないので、使われなくなった配置が残ることがある。番号は1回の変換の中だけで意味を持つ。VM は実行時に配置の番号を使わないので、REPL の追記も配置の番号を引き直さない ([VM](vm.md) の「追記」)
- extern が作るデータは、比べる extern と `Std.Env.has`、`Prelude.shows_minus`、`Std.Float.can_parse`、`Std.IoTest.op_timed_out` の `Bool`、`compare` の extern の `Ordering` である。`Bool` は機械の定数 `FALSE` と `TRUE` で、`Ordering` は `LT`、`EQ`、`GT` で作る。そのタグが `Prelude.Bool` と `Prelude.Ordering` の宣言の順と合うことを、テストが確かめる。そのため、extern の行は配置を持たない。組は extern が作らず、`with_arg` の行の組は translate が作る (下の「組を返す extern」)。名前的なデータ (`List`、`Option`、`Result`) は extern が作らず、標準ライブラリの eml が包む ([標準ライブラリ](stdlib.md) の「extern の境界」)
- 名前付きのレコードは、ほかの `data` と同じく、型ごとの配置に入る。名前付きのフィールドを持つコンストラクタも、フィールドの名前は持たず、宣言の順のフィールドの Repr の列を持つ

### 組を返す extern

- `Std.MutArray.length` と `read` は、宣言の型では結果と配列の組を返す (`MutArray a -> Int -> <Panic> (a, MutArray a)`)。extern は借りない引数を消費するので、組を作る extern にすると、読むたびに組を確保する。そこで行は配列を借り (`FunctionRow::borrowed`)、要素だけを返し、`with_arg: Some(0)` を持つ ([LIR](lir.md) の「extern の借用」)
- `Std.FileIn`、`Std.Stdin`、`Std.Proc` の読み (`read_all`、`read_line_raw`、`read_all_bytes`、`read_line_bytes_raw`、`read_chunk`) も同じ形である。宣言の型は読んだ結果とハンドル (`FileIn`、`Stdin`、`Child`) の組を返し (`FileIn -> <IO> (String, FileIn)`)、行はハンドルを借りて結果だけを返す。`with_arg` はハンドルの位置で、`read_chunk : Bytes -> FileIn -> <IO> (Bytes, FileIn)` (`Stdin` と `Child` も同じ形) だけは `Some(1)`、ほかは `Some(0)` である
- `Std.Async.task_cell : Task a -> (Cell, Task a)` も同じ形で、ハンドルを借りてセル (`int`) だけを返し、`with_arg: Some(0)` を持つ。ハンドルは `Lin` なので行の引数の印は `lin` で、この引数は借りたうえで預かる引数でもある ([宣言](declarations.md) の「`extern`」)
- `Std.IoTest.peer : Stream -> <IO> (Stream, Stream)` と `op_stream : Async.OpId -> <IO> (Stream, Async.OpId)` も同じ形で、`Lin` の引数を借りて新しい端だけを返し、`with_arg: Some(0)` を持つ。`op_timed_out : Async.OpId -> <IO> (Bool, Async.OpId)` も token を借りて `Bool` (`enum`) だけを返し、`with_arg: Some(0)` を持つ。3つとも引数を預からない ([標準ライブラリ](stdlib.md) の「`Std.IoTest`」)
- `Std.FileOut`、`Std.Stdout`、`Std.Stderr` の書き込み (`write`、`write_bytes`、`write_line`) は、書くデータを借り、ハンドルを消費して同じハンドルを返す。結果がハンドルそのものなので、組を返さず、`with_arg` を持たない
- translate は、`with_arg: Some(k)` の行の引数のそろった呼び出しを、行の結果を束縛する `extern` 命令と、その結果と引数 k を組にする `con` にする。`let t.1: int = extern Std.MutArray.read(m.0, 0)` と `let d.2: (,){int, obj lin} lin = con (,) #0(t.1, m.0)` の形である。組が構造体にならない型なら、`d.2` は `obj` である。extern は m.0 を借りるだけなので、m.0 を消費するのは `con` だけである。`$externN` も同じ形の本体を持つ。どちらも同じ関数 (`ProgramBuilder::extern_call`) が作る
- 組を受けてすぐ開く所 (`let (v, m) = MutArray.read m i`) では、最適化の既知のコンストラクタ (下の「既知のコンストラクタ」) で組が消え、確保が残らない。組が構造体なら、既知のコンストラクタに頼らなくても確保がない

### 変換の規則

- extern の関数の引数のそろった呼び出しは、`extern` 命令 (`Rhs::Extern`) にする。extern は、その場で実行して値を返す1階の命令で、eml のコードを呼び返さず、継続のフレームも積まない。ただし、`try_io` が捕まえる種類の誤りで失敗すると VM が `io_failed` の節の入口を呼び、`Panic` を起こす誤りで失敗するか `panic` を呼ぶと `panicked` の節の入口を呼び、`exit` は中断を始める ([VM](vm.md) の「extern の中断」)。`mask` は持たない。引数の所有権は、コンストラクタの呼び出しと同じく命令に移る。どの extern があり、引数と結果の Repr が何で、純粋かどうかは処理系の表 (`eml_extern`) が決める。`Std.Par.par` だけは `extern` 命令にせず、`par` の呼び出しにする (下の「`par` の呼び出し」)。
- 文字のリテラルは、式では `Int` の定数 (`Atom::Int`)、パターンでは `Int` の case (`CasePattern::Int`) にする。値はスカラー値のコードポイントで、`Char` は `Int` と同じ `int` の Repr を持つので ([LIR](lir.md) の「値の表現」)、リテラルの `switch` も `Int` と同じになる。`Byte` のリテラルも、値 (0 から 255) を `Int` の定数と case にする。元の型は instance の名前に残る。
- 表面の構文には `perform` はない。HIR から Core IR への変換で、操作の呼び出しを `perform` 命令に変える。
- クロージャは「関数 + すでに渡された先頭の引数の並び」で表す。ラムダは、捕まえた変数を先頭の引数に持つ関数に持ち上げる。ラムダの捕獲と部分適用は、どちらもこの形のクロージャになる。
- 関数値の呼び出しは、実行時に引数の個数を比べる eval/apply 方式で行う。個数が揃えば呼び、足りなければ引数を足した新しいクロージャを作り、余れば先頭の分で呼んだ結果に残りを適用する。呼ぶ相手が分かっている呼び出しは、変換の時点で同じ場合分けをする。
- 捕まえた変数がなく、部分適用の引数もない関数は、クロージャを確保せず、関数の値 (`Atom::Fn`) にする。関数の値はヒープに置かず、RC の対象にならない。`closure` (`Rhs::MakeClosure`) はつねに1つ以上の引数を持ち、同じ値に2つの書き方を残さない。関数の値の呼び出しも、引数のないクロージャと同じ eval/apply で行う。
- 引数のないトップレベルの値は、参照するたびに計算する。値は純粋なので結果は変わらない。一度だけ計算する形はない ([ロードマップ](../future/roadmap.md))。
- コンストラクタのタグは、`data` の宣言の順の番号で、その型の配置の中の添字である。引数のないコンストラクタの値は即値のタグで、引数を持つコンストラクタの値はヒープのオブジェクト (`Payload::Data`) である。`Bool` は Prelude の `data Bool = | False | True` で、`False` のタグが 0、`True` のタグが 1 になる。IR の定数 `FALSE` と `TRUE` はこの値を持ち、`eml_core_ir` のテストが Prelude の宣言のタグと一致することを確かめる。`Ordering` の定数 `LT`、`EQ`、`GT` (タグ 0、1、2) も同じく確かめる。
- タプルの値は、タグ 0 のコンストラクタの値と同じオブジェクトで表す。配置は大きさごとの `(,)`、`(,,)` である。`Unit` (要素のないタプル) は即値の `()` である。
- リストのリテラル `[e1, …, en]` は、要素を左から評価して変数に入れた後、`Nil` のタグから始めて、右の要素から順に `::` の `con` を積む。ループで組むので、要素が多くても変換のスタックは深くならない。`[]` は、コンストラクタ `Nil` の参照と同じタグの定数 (`Atom::Tag`) である。`Prelude.List` の配置は、ほかのデータ型と同じく、IR が初めて使ったときに表に入る
- レコードの作る式は、フィールドの式を書いた順に評価して変数に入れ、宣言の順に並べ替えて `con L #t(…)` を作る ([式](expressions.md) の「レコード」)。配置はほかの `data` と同じで、名前付きのフィールドのためだけの配置や命令はない
- 射影 `e.f` は、`e` を評価し、`unpack v L #t(f0, …, fn)` で分解して、`f` の位置の変数を結果にする。タプルの `t.0` も同じ形で、配置は組の配置である。`unpack` の変数の Repr は、取り出すフィールドの変数だけ射影の式の型の Repr にし、ほかのフィールドは配置のフィールドの Repr にする。型変数のフィールドの `tobj` から `int` への `unbox` などの変換は、box の挿入が足す ([LIR](lir.md) の「box の挿入」)。取り出したフィールドは、ほかのフィールドと同じく借りて始まる。読むだけなら RC の命令は付かず、消費するなど所有が要る所で、Perceus が `dup` するか、値を `release` で解いて参照を引き継ぐ ([LIR](lir.md) の「Perceus」)。借りたまま読む専用の命令は足さない。`f p.name` の後に `p` を使う形は、呼び出しが取り出したフィールドを消費するので `dup` が要る。これを減らすのは借用パラメータである ([ロードマップ](../future/roadmap.md) の「段の列」)
- 射影と更新で、`unpack` する値の印が `unr` でなく、`unpack` のフィールドの印がどれも `unr` なら、`unpack` の直後に `drop v` を置く。`Lin` と宣言した型のフィールドが `unr` だけの値 (`data Ledger : Lin = | Ledger { entries : List Int }` の `l.entries`) である。Perceus はフィールドの消費で v を `release` で解かないので、残る `decref` を V1 が拒むためである。上の「決定木の葉の `drop`」と同じ規則で、`drop` の後で使う `unr` の RC のフィールドは、Perceus が `dup` で所有を取る
- 更新 `{ e | f = v }` は、`e` を評価し、フィールドの式を書いた順に評価してから、`unpack` で古いフィールドを取り出し、置き換えるもの以外の古いフィールドと新しい値を宣言の順に並べて、新しい `con` を作る。`unpack` の変数の Repr は、残すフィールドの変数を、型引数で具体化したフィールドの型の Repr にする。作る式の `con` の引数と同じく、後の決定木はこの `con` の引数をパターンの変数にそのまま渡すためである。置き換えるフィールドの変数は、配置のフィールドの Repr のままにする。意味は写しで、残したフィールドは Perceus が所有にし、上書きされる古い値と元の値は手放す。一意な値をその場で書き換えるのは、Perceus の reuse ([LIR](lir.md) の「reuse」) に任せる
- レコードの分解のパターンは、HIR がコンストラクタのパターンに組むので、決定木の規則は変わらない。フィールドのセクションは、ほかのラムダと同じく変換する
- 穴のある文字列は、本文と穴を左から1つずつ処理する ([式](expressions.md) の「補間」)。最初の部分 (本文の文字列の定数か、穴の結果) を、組み立て中の文字列にする。それ以降の部分は、評価するたびに `extern` 命令の `Prelude.++` (`Extern::StrConcat`) で組み立て中の文字列につなぐ。部分が1つだけなら (`"\{x}"`)、連結はしない。連結の命令は、導出した `Show` と同じく表の行を直接指し、位置を持たない。ユーザーが自分の `++` を定義しても変わらない
- 評価の途中でつなぐので、穴の呼び出しをまたいで生きているのは組み立て中の文字列だけである。リストのリテラルのように、すべての部分を評価してから組むと、呼び出しをまたいで生きている変数の数の合計が部分の数の2乗になる
- 最初の連結は、左辺が不死のリテラルか共有された文字列なら、写して新しい文字列を作る。2つ目からは、一意な左辺をその場で伸ばす ([オブジェクトモデル](object-model.md) の「文字列の操作」)。そのため、全体の長さに比例する時間で済む
- コンストラクタを部分適用したときと関数値として使ったときは、コンストラクタの値を作るだけの関数 (`$con$…`) で包む。包む関数は参照の型ごとの instance で、名前に型引数を書く (`$con$Pair@[Int, Int]`)。引数と結果の Repr を具体化した型から決めるので、型ごとに分けないと、名前が同じ関数が違うコードになるためである。
- クラスのメソッドへの参照は、instance のメソッドの関数、既定のメソッドの関数、`extern` 命令、生成した関数のどれかに変換する (下の「メソッドの解決」)。
- extern の関数を値として使うとき (引数が足りないときを含む) は、参照する場所ごとに、それを呼ぶだけの関数 `<外側の関数>$externN` で包む。本体は `let t.1: unit = extern Prelude.println(p.0) @"main.em":6:9` と `return t.1` のような形で、`extern` 命令はその参照の場所の位置を持つ (下の「位置」)。instance の `extern` で結んだメソッドを値として使うときも、同じく包む。包む関数の型は参照の型から取り、名前は外側の instance の名前を根にする。メソッドの参照が extern になるかは instance によるので、`$externN` の番号は、同じ本体でも instance ごとに変わりうる。
- `if c then a else b` は `match c with | True -> a | False -> b` と同じ分岐にする。条件 (`&&`、`||`、入れ子の `if`、条件の位置の `match`) は `Bool` の値を作らずに分岐し、短絡評価の順を保つ。値が静的に決まる条件は、`switch` を出さずにその枝だけを変換する。
- 末尾にない分岐の値は、引数を持つ合流のブロックで受ける。各枝は値を `jump` で合流のブロックに渡し、合流のブロックが続きを実行する。値を返す枝が1つだけなら、合流のブロックを作らず、その枝のブロックで続きを変換する。末尾の分岐は合流のブロックを作らず、各枝が直接返す。
- `match` は決定木にコンパイルする。決定木は、最初の行でコンストラクタ、タプル、リテラルのどれかのパターンを持ついちばん左の列を選ぶ。コンストラクタの列は、その列の値で `switch` する。列のどの行にも現れないコンストラクタは、case を作らず `default` にまとめる。リテラルの列は、列に現れる異なるリテラルを case に持つ1つの `switch` にし、リテラルに当たらない値の行列を `default` で続ける。リテラルを比べる命令の連なりにしないのは、枝の数だけ入れ子を深くしないためである。`match` は網羅性の検査を通っているので、どの枝にも当たらない値の受け皿は作らない。
- コンストラクタが1つでフィールドを持つ型 (タプルを含む) の列は、`switch` ではなく `unpack` で分解する。フィールドのない唯一のコンストラクタは、`()` と同じくワイルドカードとして扱い、何も出さない。
- scrutinee がタプルのリテラルなら、タプルを作らず、その要素を決定木の列にする。複数の等式を持つ関数で HIR が作るタプルも含む。値全体を束縛する行があれば、その葉でだけタプルを作る。
- 決定木の葉が1つの枝にだけ届くとき、その枝の本体は葉のブロックに置き、パターンが束縛する変数を葉の値で置き換える (別名の `let` を作らない)。複数の葉から届く枝は、パターンが束縛する変数を引数に持つ合流のブロックにし、葉から `jump` する。どの葉にも届かない枝は出さない。
- `let`、ラムダの引数、等式の引数のパターンも、同じ決定木で分解する。葉が1つなら今のブロックに `unpack` を並べ、続きもそのブロックで変換する。葉が複数なら、葉から続きのブロックへ `jump` し、束縛した変数をその引数にする。
- 頭のコンストラクタかリテラルが変換の時点で分かっている値 (コンストラクタの適用、タプルのリテラル、タグと `Int` の定数、同じ関数の中で `let x = con ..` で束縛した変数) を調べるときは、`switch` を出さずにその場で case を選ぶ。ただし印が `unr` でない `con` の変数は、上の「Kind の印」の開く条件に当たるときだけ開き、そうでなければ変数そのものを分解する。
- `match`、分解する `let`、条件の scrutinee が分岐 (`if`、`match`、それで終わるブロック) なら、分岐の各出口の値を `match` に直接渡す (case-of-case)。`let x = <分岐>` の直後の本体が `match x` である形も含める。出口の値で1つの枝に決まれば、コンストラクタを作らずにその枝へ `jump` し、決まらない出口の値だけを合流させて `switch` する。束縛と `match` の間に文がある形と、呼び出しの結果は扱わない。
- 関数の末尾の呼び出しは末尾呼び出しにし、呼び出し元のフレームを積まない。末尾の位置の呼び出しの定義と保証は [LIR](lir.md) の「末尾の位置の呼び出し」にあり、末尾呼び出し (`tail`) は LIR の縮約だけが作る。translate は、値を返すだけの合流のブロック (文がなく、`return p` だけを持ち、引数が `p` だけ) へ向かう `jump` を `return` に置き換え、`let x = <呼び出し>` と `return x` を残す。translate の出力では `jump` の実引数と行き先の引数の Repr が同じなので、この転送は Repr に関わらない。印の受け渡しは推移的でないので、実引数が定数か、実引数の印が行き先の引数の印と同じときだけ転送する。葉、出口、合流の両側の印は同じ型から決まるので、translate の出力では転送は変わらない。対象の呼び出しは、直接の呼び出し、`mask` で分けた `apply` の最後の部分、余った引数を渡す呼び出し、`handle`、`perform`、`resume`、引数のないトップレベルの値である。EP の後は、`clause` も対象になる。
- 関数のコードは instance ごとに変換する (単相化)。instance は、定義された関数、型引数の列、row のビットの組で、この組を instance の鍵と呼ぶ。型引数は型の表の `TypeId` で、シグネチャの型変数の順 (`generics.type_vars` の順) に並ぶ。型変数を持たない関数の型引数は、空の列である。row のビットは、row 変数ごとに「yield しうる row が入ったか」を持つ (下の「row のビットと instance の鍵」)。変換は、入口から届く instance だけを Core IR にする。標準ライブラリのうち使わない関数は Core IR に入らない。
  - 鍵は型そのものである。`List Int` と `List String` のように Repr が同じ型引数も、関数型の row だけが違う型引数 (`Unit -> <e> Int` と `Unit -> <IO> Int`) も、別の instance になる。型クラスの証拠が型ごとに変わるためである。Repr が同じ instance はまとめない ([ロードマップ](../future/roadmap.md) の「処理系」)
  - 本体の中の関数への参照 (呼び出しと、値としての参照の両方) は、型検査が参照ごとに記録した型引数を持つ。呼ぶ先の鍵は、この型引数に、参照を含む instance の代入をかけて作る。instance の代入 (`TypedProgram::substitution`) は、その instance の関数自身のシグネチャの型変数を instance の型引数に置き換え、宣言の Kind 変数 `Var(i)` を `TypedProgram::kind_values` が求めた値 (`Unr`、`Lin`、`Poly`) に置き換え、handler の節の操作ごとの型変数 `OpVar(名前, k)` をいつも `Flexible(k)` に置き換える ([型と Kind](types.md) の「instance での評価」)。row の末尾は変えない。その後、呼ぶ先の一様な位置 (次の項目) の型引数を、参照の型引数によらず `Flexible` にする。この `Flexible` の Kind は、関数なら `TypedProgram::uniform_kind` が呼ぶ先のスキームから決める (`Unr` に固定されていれば `Unr`、そうでなければ `Poly`)。導出とタプルの生成器の鍵では `Poly` である。残った `Flexible` (節の型変数から来たもの、型検査で最後まで決まらなかったもの) は、Kind を持ったまま鍵に入る。`Flexible` の Repr は `tobj` である。extern の関数、操作、コンストラクタへの参照は instance を作らない
  - 多相再帰 (`depth : Int -> a -> Int` の本体が `depth (n - 1) (x, x)` と呼ぶ形) は、型変数の流れのグラフで見つけ、その位置を一様な位置にする。グラフは型検査 (`eml_types`) が型検査の後に計算して `TypedProgram::uniform` に入れ、変換はそれを読む。この項目は関数の節点と関数の参照の辺だけを書く。クラスのメソッドと instance の節点と辺は、下の「一様な位置と制約付きの多相再帰」にある。グラフの節点は (関数, 型変数の番号) で、HIR のすべての定義された関数について作る。関数 f の本体の参照 `g @[T0, T1, …]` で f の型変数 i が Tj に現れるとき、(f, i) から (g, j) へ辺を引く。「現れる」は、型構成子の引数、タプルの要素、関数型の引数と結果とエフェクトの型引数のどこかにあることである。Tj が i の型変数そのものでなければ、その辺は大きくなる辺である。大きくなる辺の両端を含む強連結成分を大きくなる成分と呼び、その節点を一様な位置とする。自分自身への大きくなる辺を持つ節点 (`depth` の型変数) も、大きくなる成分である
  - 一様な位置は、関数ごとでなく型変数の位置ごとに決める。`walk : Int -> a -> b -> b` が `a` だけを大きくして自分を呼ぶとき、`walk` を `b := Int` で使った instance は `walk@[_, Int]` で、`b` の位置の引数と `ret` は `int` である。大きくならない型変数の制約の証拠を、具体的な型から確定するためである
  - 一様な位置の型引数は `Flexible` なので、大きくなる成分を通る型の大きさは有界になる。大きくなる成分の外の位置に流れ込む型も、有界な型と `Flexible` から作られるので有界である。そのため、instance を集める手順は止まる。instance の総数に上限は置かない ([実装の現在地](../implementation/status.md) の「コンパイルの時間と深さ」)
  - 入口の関数の鍵は、型変数を持たなければ空の列、持てばすべて `Flexible` である。その Kind は、一様な位置と同じく `uniform_kind` が決める。入口は `main` とは限らず、変換の呼び出し側が選ぶ
  - ランタイムが名前で呼ぶ関数 (`Std.Async.start_task` と `Std.Async.resume_task`。[標準ライブラリ](stdlib.md) の「`Std.Async`」) も根にする。eml のコードは呼ばないので、入口から届かないためである。そのモジュールを読み込んでいるときだけ、`モジュール名.関数名` で引ける関数を、型引数も row のビットも空の instance にする。どれも単相なので、名前は元の名前のままである
  - instance は、入口の instance から始めて、見つけた順に処理する (FIFO)。各 instance の本体では、関数への参照を HIR の式の ID の順にたどり、まだない鍵を列の後ろに足す。すべて集めてから、instance を HIR の関数の順に並べ、同じ関数の instance は見つけた順に並べて、その順に関数の番号を振る。`TypeId` の値の順には並べない。後の段階が `TypeId` の値に意味を持たせないためである。型変数を持たない関数だけのプログラムでは、関数の並びは HIR の関数の順になる
  - 型変数を持つ関数の instance の名前は、元の名前 (モジュールの修飾を含む) に `@[型引数, …]` を付けた形である (`map@[Int, String]`、`Prelude.>>@[Int, Int, Int]`、`length@[List Int]`)。一様な位置の型引数 (`Flexible`) は `_` と書く (`depth@[_]`)。矢印の線形性は、`Lin` を `once a -> b`、`Poly` を `poly a -> b`、`Unr` を `a -> b` と書く ([型と Kind](types.md) の「Kind の書き出し」)。線形性だけが違う具体化は、別の instance として別の名前になる (`id@[once Int -> Int]` と `id@[Int -> Int]`)。一様な位置と操作ごとの型変数も Kind を書く (`_:poly`、`a:lin`。`Unr` は書かない)。Kind だけが違う鍵の名前を重ねないためである。名前は型引数だけで決まり、ほかの instance の有無や見つけた順によらない。REPL の追記が、名前が同じ関数はいつも同じコードになることに頼るためである ([VM](vm.md) の「追記」)
  - 型引数の型とクラスの名前は、名前のための表 (`NameTable`) で書く。診断の表示名は名前が重なるときだけ修飾するので、ほかのモジュールや後の入力が同じ名前を定義すると表示が変わり、名前が型引数だけで決まらなくなるためである。表は、Prelude の型とクラスを修飾しない (`Int`、`Show`)。Prelude は固定なので、修飾しなくても重なりは入力で変わらない。入口のモジュールと REPL のセッションの型とクラスは中段の名前 (下の「REPL の入力」) で書き、Prelude の型かクラスと同じ名前のものは `Main.` で修飾する (`Main.Bool`)。ほかのモジュールの型とクラスは、モジュール名で修飾する (`Std.FileIn.FileIn`)
  - `@[` と `]` の間が 64 文字を超える instance は、`@[…]` の代わりに、型引数の構造から計算する 64 ビットのハッシュを16桁の16進数で `@#` の後に書く (`ident@#3fa91c0d5e2b7a10`)。部分を共有する型の型引数で、名前が指数の長さになるのを防ぐためである。ハッシュは、表示と同じ情報 (名前、矢印の線形性、row、Kind) を、型の DAG の節ごとに1回だけ計算する。`TypeId` にはよらない。debug ビルドは、プログラムの中で名前が重ならないことを確かめる。入力をまたいだハッシュの衝突は、REPL の debug ビルドの一致の確かめだけが捕まえる。64 ビットの衝突は起きにくいので、release ビルドでは受け入れる
  - 型変数を持たない関数の名前は、元の名前のままである。立ったビットの row 変数は、名前の後ろに `<e>` の形で書く (下の「row のビットと instance の鍵」)。ラムダ、handle の本体と節、`var` の範囲と節、`$externN` の名前は、instance の名前を根にする (`twice@[Int]$lambda0`、下の「`var` の変換」)。ローカル関数の名前は `<instance の名前>$let$<ローカル関数の名前>` で、同じ instance の中で名前が重なるときは、HIR の順で2つ目から `#2`、`#3` を付ける (`map@[Int, Int]$let$go`、`$let$go#2`。下の「ローカル関数の持ち上げ」)。入口を呼ぶ関数の名前は、instance の名前でなく元の名前から作る (`entry$first`)。位置を持たない実行時エラー ([LIR](lir.md) の「実行時エラー」) は、instance の名前を出す
  - instance の本体は、本体の型 (式、局所変数、パターン、参照の型引数) に instance の代入をかけた型で変換する。代入をかけた後には型変数も Kind 変数も残らない (debug ビルドで確かめる)。型変数を持たず、定数に畳まれない宣言自身の Kind 変数も持たない関数は、本体の型を写さずに型検査の表をそのまま使う。このとき本体の型に現れる変数は節の `OpVar` だけで、その Kind は定数である。写さない関数の本体に Kind 変数が残っていないことも、debug ビルドで確かめる。トップレベルの関数の引数と `ret` の Repr は、宣言の型に代入をかけた型から決める。メソッドの解決も、代入をかけた型引数から行う。ラムダ、ローカル関数、handle の本体、節は、外側の instance の型を使う。節の型変数は `Flexible` になるので、節の中のコードは一様なままである
  - データの配置は、instance によらずスキームから作る。データの配置まで型ごとに分けると、一様な位置を通るコードと配置が食い違うためである (上の「translate の配置」)。`$op$`、`$con$`、`$cont$`、`$cont$state` は、参照か `k` の型ごとの instance である (`$op$read@[Int]`、`$cont$state@[Int, Int, Int]`)。型変数のないスキームでは名前に型引数を書かない
- テキストの形 ([テスト戦略](../implementation/testing.md) の「Core IR のテキストの形」) は関数とエフェクトを名前で引くので、モジュールをまたいで名前が重なってはいけない。入口以外のモジュールでは、関数、操作を包む関数、コンストラクタを包む関数、エフェクトの表の名前に `モジュール名.` を付ける (`Report.Csv.parse`、`$con$Report.Csv.Row`)。標準ライブラリのモジュールもこの規則に含め、正式な名前を使う (`Prelude.not`、ラムダは `Prelude.>>$lambda0`)。入口のモジュールの名前には付けない (`parse`、`$op$get`)。extern の呼び出し (`extern <正式な名前>(…)`) は、正式な名前をそのまま使う (`extern Std.FileIn.open`)。
- 持ち上げた関数の番号 (`$lambdaN`、`$handleN`、`$externN` の N) は、外側の関数の本体の中で、HIR の式の ID の順に振る。変換の順に依らないようにするためである。ローカル関数の定義のラムダは `$lambdaN` の番号を使わない。
- 変換は、`main` を `()` で呼ぶ入口の関数を作る。等式に引数のない `main` は関数値を返すので、入口の関数が返った値に `()` を適用する。
- `handle`、`perform`、`resume` は、translate の出力では呼び出しの一種である。表面の構文に `resume` はなく、`resume` は変換が作る Core IR の内部の命令 (`Call::Resume`) である。EP がどれも基本操作と普通の呼び出しに置き換える (下の「evidence passing」)。
- 呼び出し (`Rhs::Call` の `Call::Direct`、`Call::Apply`、`Call::Resume`、`Call::Par`) は `mask` を持つ ([エフェクトと handler](effects.md) の「handler の意味」)。`mask` は、呼び出しの間に飛ばす handler を表す項目の並びである。項目 (`eml_lir::MaskItem`) は、エフェクトの番号と、残す数 n と、飛ばす数 m を持つ。m は 1 以上である。項目はエフェクトの番号の真に昇順に並ぶので、同じエフェクトの項目は2つない。空なら `mask` なしである。extern のエフェクトは入れない。`Call::Handle` と `Call::Perform` は `mask` を持たない。LIR の呼び出しは `mask` を持たない。`handle` の本体の中の呼び出しは、持ち上げた本体の関数の中の呼び出しとして `mask` を持つ。
- 変換は、型検査が呼び出しの矢印ごとに記録した `mask` の項目 ([型と Kind](types.md) の「推論」) を、エフェクトの番号に写して昇順に並べ、Core IR の呼び出しに付ける。型検査の項目は、エフェクトが rest に初めて現れた順に並ぶ。
  - 引数がそろう既知の関数の呼び出し (`Call::Direct`) は、最後の矢印の `mask` を使う。前の矢印は部分適用で、エフェクトを起こさないためである。同じ理由で、引数が足りずにクロージャを作るときは `mask` を付けない。extern、操作、コンストラクタの呼び出しには、型検査が `mask` を記録しない。ただし `Std.Par.par` は結果の row が開いているので、型検査が最後の矢印に `mask` を記録する (下の「`par` の呼び出し」)。
  - 余った引数を `Call::Apply` で渡すときは、矢印ごとの `mask` が変わる境目で `apply` を分ける。`mask` は1つの Core IR の呼び出し全体に効くので、違う `mask` の矢印を1つの `apply` にまとめると、片方の矢印に余計な `mask` が効くためである。yield しうる矢印の後でも分ける (下の「`apply` の印と分け方」)。
  - `mask` が効くのは呼び出しそのものだけで、引数の評価には効かない。Core IR の呼び出しの引数は値 (アトム) なので、引数は呼び出しより前の文で評価し終えている。そのため、これは形から決まる。
- `mask` は translate の出力にだけある。EP が `evv_mask` の包みに置き換える (下の「`mask` の包み」)。包みは戻りで `evv` を戻すので、`mask` 付きの末尾の位置の呼び出しは末尾呼び出しにならない。
- 変換の段の verifier は、`mask` のエフェクトの番号がエフェクトの表にあること、番号が真に昇順に並んでいること (同じエフェクトの項目が2つないこと)、どの項目も m が 1 以上であること、`mask` が `handle`、`perform`、`clause` に付いていないことを確かめる (``a mask is not in ascending order``、``a mask item of `State` skips no handler``)。EP の後の段は、`mask` が残っていれば誤りにする。
- extern のエフェクト (`IO` と `Panic`) も、ほかのエフェクトと同じく Core IR のエフェクトの表に入る。番号は下の「IO の extern」にある。`mask` は extern のエフェクトを持たない。また verifier は、`extern` 命令の引数の数を表の行と比べる。引数と結果の Repr は、box の挿入の後の LIR の範囲の段と所有の段で比べる ([LIR](lir.md) の「extern の行」)。
- handle の本体、操作の節、`return` の節は、ラムダと同じく、捕まえた変数を先頭の引数に持つ関数に持ち上げ、そのクロージャか関数の値を `handle` に渡す。本体の関数は `()` を受ける。節はエフェクトの操作の順に並べる。操作を値として使うときは、`perform` を呼ぶだけの関数 (`$op$…`) で包む。`$op$` も `$con$` と同じく参照の型ごとの instance である。
- 変換は、節ごとに `k` の使い方を HIR で調べ、次の2つの形のどちらかにする。
  - 直接の形: 節の本体のどこでも (入れ子のラムダや handle の本体の中を含む)、`k` の使用がすべて引数をそろえた直接の呼び出し (`k v`、状態ありなら `k v st`) か `drop k` なら、クロージャを作らない。その呼び出しを、生の継続への `Call::Resume` にする。生の継続をラムダや handle の本体が捕まえても、`Call::Resume` で再開できる。引数が余る呼び出し (`r` が関数型のときの `k v x`) は、`Call::Resume` の結果への `apply` を続ける。`mask` は、その呼び出しの最後の矢印の記録を付ける。持ち上げたローカル関数の引数に生の `k` を渡す呼び出しは、この形に当たらない。呼び出し先の `return` のブロックが `k` の末尾の `resume` で終わらないので、節はその場で動かない (下の「ローカル関数の持ち上げ」)。
  - 包む形: `k` を関数の値として使う (渡す、しまう、部分適用する) なら、節の入口で、生の継続を継続を包む関数のクロージャにし、それを `k` とする。`k` の呼び出しは普通の関数値の呼び出し (`Call::Apply`) になる。
- 継続を包む関数は、状態のない handler 用の `$cont$` と、状態のある handler 用の `$cont$state` の2つで、使うときだけ `k` の型ごとにプログラムに1つずつ作る。名前の型引数は、再開の値と状態 (状態のある handler だけ) と結果の型である (`$cont$@[Int, Int]`)。`$cont$` は `k` と `v` を受けて `resume k(v, ())` を、`$cont$state` は `k`、`v`、`s` を受けて `resume k(v, s)` を、それぞれ末尾呼び出しする。
- `resume` (`Call::Resume`) は、継続 `k`、再開の値、状態を持つ。状態のない handler の状態は `()` である。EP がこの `resume` を `apply` にし、handler への特殊化が節の写しの `apply` に写す (下の「handler の複製」)。どちらの `apply` も一様である ([LIR](lir.md) の「関数の ABI」)。
- 包んだ `k` は、`once` なら1回しか使わないので、Perceus は `dup` / `decref` を付けず、クロージャも一意で、呼ぶと消費される。
- 包んだ `k` を `drop` するとクロージャが解放され、それが継続を解放する。
- パラメータ付き handler は脱糖しない。Core IR では、すべての handler を状態のある handler として扱い、状態のない handler は状態を `()` にする。`handle` はつねに状態の初期値を、`resume` はつねに次の状態を持つ。状態のない handler の `k v` は、直接の形なら `resume k(v, ())` に変換し、包む形なら `$cont$` を通って `resume k(v, ())` になる。この `()` は Core IR の中の決まりで、表面の構文では状態のない `k` は1引数の関数である ([エフェクトと handler](effects.md) の「パラメータ付き handler」)。節のクロージャはつねに状態を最後の引数で受け、`return` の節はつねに本体の値と状態を受ける。呼び出しの規約を1つにして、EP で状態の有無を場合分けしないためである。`return` の節はつねにある。省略した handler は、HIR が `| return x -> x` の節 (状態のある handler では、状態を `_` で捨てる `| return x _ -> x`) を合成するので、本体の値をそのまま返し、状態の引数は Perceus が捨てる (RC の対象の状態だけ decref する)。省略した節は状態を `_` で捨てるので、状態の型は `Unr` である ([式](expressions.md) の「パラメータ付き handler」)。
- メモリ管理は Perceus 方式の参照カウントである。`Lin` の値は2回以上消費されず、`dup` も受けない ([LIR](lir.md) の「値の表現」)。

#### `par` の呼び出し

- `Std.Par.par` の引数のそろった呼び出しは、`extern` 命令でなく `par` の呼び出し (`Rhs::Call { call: Call::Par { left, right, pair, at }, mask }`) にする。extern は eml のコードを呼び返さないが、`par` は2つの子の閉包を呼ぶためである。呼び出しの一種にするので、`mask` と巻き戻しの表をほかの呼び出しと同じ仕組みで扱える。`eml_extern` の `Extern::Par` の行は std の宣言を型検査に結ぶためだけにあり、`Rhs::Extern` にならない。verifier は `extern Std.Par.par` をどの段でも拒む ([LIR](lir.md) の「verifier」)
- `left` と `right` は2つの引数、`pair` は `tuple_layout(2)` の `TUPLE` の構築子、`at` は `par` の名前の位置である。結果は組の型の変数 (`obj` か構造体) にそのまま束ねる。構造体なら、box の挿入が `obj` からの `unbox` を足す ([LIR](lir.md) の「box の挿入」)。タプルの配置の欄はどれも `tobj` なので (上の「translate の配置」)、構造体の箱は VM が作る組と同じ形である
- `mask` は、型検査が最後の矢印 (2つ目の引数の矢印) に記録したものである。`par` の結果の row は開いた `<e>` なので、`foo : (Unit -> <never e> Int) -> <Fail String | e> (Int, Int)` の本体の `Par.par h h` は `mask [Std.Fail.Fail] par (,) #0(h.0, h.0)` になる。`h` が起こす `Fail` は、`foo` の `Fail` を飛ばして呼んだ側の handler に届く
- 部分適用 (`Par.par f`) と値として使う `Par.par` は、ほかの extern と同じく `$externN` で包む (上の「変換の規則」)。包む関数の本体も同じ `par` の呼び出しを、同じ関数 (`ProgramBuilder::extern_call`) で作る。本体は最後の矢印の呼び出しでないので `mask` を持たず、`mask` は包む関数を呼ぶ `apply` が持つ。結果は組で関数でないので、`par` に余る引数はない
- 最適化は `par` をほかの文と同じく扱う。yield の地点でなく、handler に触れず (下の「handler への特殊化」)、到達の解析は2つの子をたどる。never だけのエフェクトの handler は最適化で消えないので、子の中断を起こし直すとき、親はいつもその evidence を持つ。handle を消すのは handler への特殊化だけで、それは操作がどれも `never` のエフェクトを断り、展開は `handle` を持つ関数を断るためである

### ローカル関数の持ち上げ

ブロックの文 `let name apat+ = body` と、`and` で続けた組の関数 ([式](expressions.md) の「ローカル関数」) は、translate が関数に持ち上げる。直接呼ばれるために生まれる関数なので、内部の関数 (`internal`) にせず、`split` も持たない ([LIR](lir.md) の「関数の ABI」)。

- instance の本体を変換する前に、本体のローカル関数をすべて定義の順に予約して変換する。自分と組の相手への参照と、後で変換する入れ子の関数からの参照のために、関数の番号と持ち上げの捕まえる変数の表が先に要るためである。文の位置では何も出さない。ローカル関数は単相なので、外側の instance ごとに1つの関数になる
- 引数は、持ち上げの捕まえる変数を先に、宣言した引数を後に並べる
- 名前は `<外側の instance の名前>$let$<ローカル関数の名前>` である (`map@[Int, Int]$let$go`)。`let` は識別子に使えないので、ローカル関数の名前が `lambda0`、`k0` などでも、translate と EP が作る名前 (`$lambdaN`、`$handleN`、`$externN`、`$kN`、`$jN`) と重ならない。同じ instance の中で名前が重なるときは、HIR の順で2つ目から `#2`、`#3` を付ける。テキストの形では引用符で囲む
- ローカル関数の本体の中のラムダ、handle の本体、節、`$externN` は、外側の instance の名前を根にし、番号も外側の本体の中で HIR の式の ID の順に振る
- ローカル関数への参照は次のように変換する。参照するのがどの関数でも同じで、外側の本体からも、ラムダ、`use` のラムダ、handle の本体、節、別のローカル関数、自分の本体から参照しても、直接呼び出しのままにする
  - 引数がちょうどの呼び出しは、`Call::Direct` にする。持ち上げの捕まえる変数を先に渡す。`mask` は最後の矢印のものを使う。最後でない矢印の row は `<>` なので、ほかの矢印は `mask` を持たない
  - 引数が足りない呼び出しと、値としての参照は、使う位置で閉包 (`closure`) を作る。持ち上げの捕まえる変数がなければ `Atom::Fn` にする
  - 引数が余る呼び出しは、トップレベルの関数と同じく、`Call::Direct` の後に `apply` を続ける
- 持ち上げの捕まえる変数は、HIR の捕まえる変数に含まれるローカル関数を、そのローカル関数の持ち上げの捕まえる変数に置き換えたものである。並びは変数の番号の順である。組ごとに次のように求める。ローカル関数は、前の組の関数と自分の組の関数しか参照できない。HIR の組の一覧 (`Body::local_groups`) は外側の組を入れ子の組より先に並べるので、組の順に1回ずつ求めればよい。ラムダ、handle の本体、節も、捕まえる変数にローカル関数があれば同じく置き換える
  - 再帰する組 ([式](expressions.md) の「組」) の関数は、組の関数が捕まえる変数の和から組の関数を除いたものを、共通の持ち上げの捕まえる変数として受け取る。そのため、組の中の呼び出しは、受け取った追加の引数をそのまま相手に渡せる。再帰する組の関数が捕まえるのは `Unr` の値だけなので ([線形性](linearity.md) の「基本の規則」)、共通にして複製してもよい
  - 再帰しない組の関数は、組の中で互いを参照しないので、`and` のない `let` と同じく、それぞれが自分の捕まえる変数を受け取る。共通にすると、ある関数が捕まえた `Lin` の値を、ほかの関数の呼び出しも受け渡して複製してしまう
- 使用回数と持ち越しの検査と `closure_kinds`、debug ビルドの `check_captures` は、HIR の捕まえる変数を使う。入れ子のラムダがローカル関数 `go` を参照することは、`go` の1回の使用であり、`go` が捕まえた値の使用ではない。`go` の Kind はその捕まえた値の Kind 以上になるので、`go` を捕まえたラムダの Kind も同じく下から抑えられる。持ち上げの捕まえる変数を直接渡しても、所有の数え方は変わらない
- `yields` の印は、ローカル関数の最後の矢印の row から、外側の instance の row のビットで決める。row が閉じたローカル関数 (呼び出しの包含を解いて閉じたものを含む。[型と Kind](types.md) の「呼び出しの包含を解く」) は、ラベルだけで決まる。末尾が開いたままのものは、ラムダと同じく外側の instance の row のビットで分類する
- 生の `k` を持ち上げたローカル関数の引数に渡す呼び出しは、`runs_in_place` の形に当たらない。`runs_in_place` は、`return` で終わるブロックがどれも `k` の末尾の `resume` で終わることを確かめるので、この呼び出しを含む節はその場で動かない
- ローカル関数を値として使っても、ABI は変わらない。値の参照は、box の挿入が作る一様な版 `<f>$uniform` を指す ([LIR](lir.md) の「関数の ABI」)
- 自分への末尾呼び出しは、トップレベルの関数と同じく contract が `tail` にする

次の3点は、この方式で受け入れた代償である。

- 値としての参照は、使う位置ごとに閉包を作る。別の関数のループの中でローカル関数を値として渡すと、反復のたびに閉包を作り直す。トップレベルの補助関数に閉包を引数で渡す書き方なら、作るのは1回である
- ローカル関数を呼ぶラムダは、呼ぶローカル関数の捕まえる値をすべて捕まえる。ローカル関数の閉包を1つ捕まえるより、閉包の語と `dup` が増えることがある。EP の継続の閉包 (`f$kN`) も同じである
- ローカル関数は、外側の instance ごとに複製する。外側の型引数に関わらないローカル関数でも、外側の instance の数だけできる

### `var` の変換

`var x = e` のノード ([式](expressions.md) の「`var`」) は、地点の合成のエフェクト L を扱う状態つきの handle にする。状態の初期値は `e` である。操作の表、節、`perform` は、型検査が決めた変数の分類 (`BodyTypes::vars`。[型と Kind](types.md) の「`var` の変数」) から組む。

- 初期値は範囲の外で、範囲を持ち上げる前に評価する。範囲 (`var` の文より後ろの文と最後の式) は、handle の本体と同じく、捕まえた変数を先頭の引数に持ち `()` を受ける関数に持ち上げる。範囲の関数はいつも `yields` である
- 関数の名前は instance の名前を根にし、範囲は `<instance の名前>$var<N>`、節は `$var<N>$<操作>`、`return` の節は `$var<N>$return` にする (`main$var0$get`)。N は本体の中の変数の番号 (`Body::vars` の順) である
- エフェクトの名前は HIR の項目の名前 `var$<k>$<x>` を、ほかのエフェクトと同じくモジュールで修飾する。セッションのモジュールでは、前の入力が同じ名前を定義していれば `#N` を足す (下の「REPL の入力」)。地点ごとに名前が違うので、VM がエフェクトを名前で引いても、外側の地点の閉包が内側の地点の handler に届かない
- 操作の表は地点ごとに1つで、Repr と印を変数の宣言の中の型 `s` (instance の代入をかけない型) から決める。型変数の位置は `tobj` と `poly` になり、instance ごとの Repr との変換は box の挿入が入れる。操作の番号は次の表の並びの位置である

  | 変数の分類 | 操作 |
  |---|---|
  | `Unr` の変数 | `get(unit) -> s`、`set(s) -> unit` |
  | 配列の変数 (`s` は `MutArray e` で、`obj lin`) | `take(unit) -> s`、`length(unit) -> int`、`read(int) -> e`、`write(int, e) -> unit` |

- 節は HIR に元がないので、translate が直接組む。引数は HIR から持ち上げた節と同じく、操作の引数、`k`、状態の順である。`k` は `tobj lin` で、どの操作も1回だけ再開する。状態の引数は初期値の型 (instance の代入をかけた変数の型) の Repr と印を持ち、`return` の節の状態の引数と同じである。節の本体は `mask` のない末尾の `resume k(v, s)` で終わり、`k` をそこで1回だけ使う。状態を渡す形の特殊化が受け付ける、すぐに再開する節の形である (下の「handler の節の形」)
  - `get` は `resume k(st, st)`、`set` は `resume k((), v)` である
  - `take` は `let h = prim empty_mut_array()` の後に `resume k(st, h)` である。取り出した後の状態は確保しない不死の空の配列になる ([LIR](lir.md) の「基本操作」)
  - `length` と `read` は `Std.MutArray.length` と `read` の行を、上の「組を返す extern」と同じ形で配列を借りて呼び、組を `unpack` で開いてから、値と配列で再開する。組は融合の既知のコンストラクタで消える
  - `write` は `Std.MutArray.write` の行で書き、`resume k((), st2)` である
- `return` の節は値と状態を受けて値を返す。`Unr` の変数は状態を捨てる。配列の変数は残った状態を `drop` してから返す
- 変数を使う所は、どれも L の `perform` にする。型検査は変数の使いを閉じた row で含めるので、ほかの操作の `perform` と同じく `mask` を記録しない
  - `x` は、`Unr` の変数なら `get` を、配列の変数なら `take` を `perform` する。`x <- e` は、`e` を評価してから `set` を `perform` する
  - 配列の変数の `x[i]` は `i` を評価し、`length` を `perform` し、`extern Prelude.check_index(i, n)` で範囲を確かめてから、`read(i)` を `perform` する。`x[i] <- e` は `i`、`e` の順に評価し、同じく範囲を確かめてから `write(i, v)` を `perform` する。`check_index` の位置は添字のノードの先頭である
  - `Array` の値の添字 (`IndexTarget::Array`) は、左側が変数なら `get` を `perform` し、式なら評価してから、`i` を評価し、`Std.Array.get` の行を直接呼ぶ。そのため `Std.Array` を読み込まなくてよい
- 範囲の外の `Panic` は、使う所の `check_index` が起こし、節の中では起こらない。節の入口は状態を evidence から取り出してから本体を動かすので、節の中で `Panic` が起こると、範囲の内側の `try_panic` が捕まえた後に変数の状態が失われるためである。`Panic` は使う所から起こるので、使う所に最も近い `try_panic` に届く。特殊化した版でも同じである。`check_index` の結果を `Unit` にするのは、戻らない呼び出しの前の `drop` (上の「戻らない呼び出しの前の `drop`」) を translate が置かずに済むようにするためである
- 節の入口の形は EP の規則のままにする。`read` と `write` の節は `Panics` の extern を呼ぶので under の形になる (下の「`perform` と節の3つの形」) が、範囲は確かめてあるので中断しない

### row のビットと instance の鍵

row は「yield しうるか」だけで単相化する。handler を使わないプログラムには、EP が何も足さない。Core IR は row そのものとラベルの順序を持たず、row の情報は関数と `apply` の「yield しうるか」の印 (下の「`yields` の印」と「`apply` の印と分け方」) だけである。

- 型検査は、参照ごとの row 変数の具体化を、yield しうるかの分類 `RowYield` として記録する ([型と Kind](types.md) の「row の具体化の記録」)
- 参照の row のビットは、参照の `rows` の各要素を、参照を含む instance のビットで解いたものである。`Always` は立つ、`Never` は立たない、`Var(i)` は含む instance の i 番目のビットに従う
- メソッドの参照の row 変数は、メソッドのシグネチャの row 変数である。instance のメソッドの関数と既定のメソッドの関数は、メソッドのシグネチャをそのまま row 変数に持つので、ビットを同じ順に写す。導出とタプルの生成器が作る関数は row 変数を持たない
- 入口の instance のビットは、すべて立てる。安全側の選び方で、`main` は row 変数を持たないので影響しない
- ビットの組は、row 変数が k 個なら 2^k 通りなので、instance を集める手順は止まる。一様な位置の議論 (下の「一様な位置と制約付きの多相再帰」) は型変数だけのもので、row には当てはめない
- 名前: 立ったビットの row 変数だけを、`@[…]` の後ろに `<e>` の形で書く。`map@[Int, String]<e>`、型引数のない関数なら `twice<e>`、ハッシュの形なら `ident@#3fa91c0d5e2b7a10<e>` である。2つ立てば `<e, f>` とシグネチャの順に書く。ビットがすべて立たない instance の名前は、ビットのない形のままである。`<` と `>` はテキストの形の名前の文字でないので、こういう名前は空白を含む名前と同じく引用符で囲む ([テスト戦略](../implementation/testing.md) の「LIR のテキストの形」)

### `yields` の印

関数 (`CoreFn::yields`) は、本体が yield しうるかの印を持つ。テキストの形では、`internal` の後に `yields fn f(..)` と書く (`internal yields fn main$lambda0(..)`)。

- translate は、関数の型の「引数の数番目の矢印」の row が yield しうるとき (`RowYield` をその instance のビットで解いて立つとき) に立てる。本体はその矢印の row で動くためである ([型と Kind](types.md) の「推論」)。`f : Int -> <State> (Int -> Int)` の `f x = …` では1番目の矢印である。引数のない値は純粋なので立てない
- トップレベルの関数は宣言の型から、ラムダとローカル関数は外側の instance の代入をかけた自分の型から決める。row が閉じたローカル関数は、最後の矢印のラベルだけで決まる。末尾が開いたままのものは、ラムダと同じく外側の instance の row のビットで分類する
- 入口の関数 `entry$<名前>` の印と、その中の `apply f(())` の印は、入口の関数の宣言の型の最初の矢印から決める。等式に引数のない入口 (`main = fn () -> ..`) では、入口の関数の instance の印 (引数の数が 0 なので立たない) が使えないためである。型検査が記録したその矢印の分類 (`BodyTypes::first_arrow_yield`、[型と Kind](types.md) の「row の具体化の記録」) を、入口の instance のビットで解く
- handle の本体はいつも立てる。本体の row は扱うエフェクトのラベルを含むので、多くの場合は yield しうる。扱うエフェクトのすべての操作が `never` のとき (`Fail.to_result` の handle など) は、そのラベルは yield しうるラベルでなく ([型と Kind](types.md) の「row の具体化の記録」)、本体の row が yield しないこともあるが、それでも立てる。印が保守的なだけで、本体に置いた yield の確かめは yield を見つけないので、意味は変わらない (この節の最後の項)。費用は、そうした本体の末尾でない呼び出しに残る確かめと、本体の中で設けた handler の節が印を写して `yields` になることだけである。印は本体の row の分類では決めない
- 操作の節と `return` の節は、handle を含む関数の印と同じにする。節は handle の外側の row で動き、その row は handle を含む関数が動いている row である
- `$op$` は、その操作を呼ぶ形なので立てる。`$cont$` と `$cont$state` は、どの文脈からも呼ばれる補助の関数なので立てる。`$con$`、導出とタプルの生成器が作る関数、`tag$`、表示関数、`$externN` は立てない。`$externN` は行が `Effectful` でも立てない。`Effectful` の extern の呼び出しは yield の地点でないためである (下の「IO の extern」)
- `optimize` は、書き換えた後のプログラムで印を付け直す (下の「最適化」の「`yields` の付け直し」)
- EP が作る関数の印は、下の「evidence passing」の各項目で決まる
- `evv` を関数の引数でなく実行の文脈に持つので ([LIR](lir.md) の「実行の文脈」)、yield しうる関数とそうでない関数の呼び出しの規約は同じである。そのため、印が保守的でも規約は食い違わない

### `apply` の印と分け方

- translate は、`mask` の変わる境目で余った引数を渡す `apply` を分けるのに加えて、yield しうる矢印の後でも `apply` を分ける。実行時の `apply` は、呼ばれる関数の引数の数で引数を分けて順に呼ぶ (eval/apply) ので、1つの `apply` の途中の矢印で yield すると、残りの引数を渡す処理が継続に入らないためである。分けた後の1つの `apply` の中では、最後の矢印のほかは yield しない
- `Call::Apply` は `yields` の印を持ち、translate は、その `apply` が受け持つ矢印のどれかが yield しうるなら立てる。分け方から、これは最後の矢印が yield しうるときである
- テキストの形では `yields apply c.0(x.1)` と書く。`mask` があれば `mask [State] yields apply …` の順にする
- `Call::Resume` から EP が作る `apply` の印は、その命令を含む関数の印と同じにする。`k` の最後の矢印の row は handle の外側の row である。生の継続をラムダや handle の本体が捕まえて再開する形でも、それを含む関数の row は `k` の row を含まなければならないので、この印は安全側である

### メソッドの解決

instance の代入をかけた後のメソッドの参照 `m@[T, U1, …]` (T はクラスの型変数の型引数、Ui はメソッド自身の型引数) は、`T` の頭で instance を引いて変換する。instance は一貫しているので ([モジュールと名前解決](modules.md) の「instance の一貫性」)、1つに決まる。証拠は型検査が記録しない。参照の型引数から、ここで引き直す ([型と Kind](types.md) の「制約」)。

| instance の種類 | 変換先 |
|---|---|
| 手で書いた instance で、メソッドを定義している | instance のメソッドの関数の instance。鍵は、頭の型引数とメソッド自身の型引数を並べた列 |
| 手で書いた instance で、メソッドを省いた | 既定のメソッドの関数の instance。鍵は `[T, U1, …]` |
| `extern` で結んだメソッド | `extern` 命令。値として使えば、extern の関数と同じく `$externN` で包む (上の「変換の規則」)。引数の数と Repr は表の行から取る |
| 導出した instance、タプルと `Unit` の instance | 中心のメソッド (`==`、`compare`、`show_prec`、`show`) は、下の「導出とタプルの生成器」が作る関数。ほかのメソッドは、手で書いた instance と同じく既定のメソッドの関数 |

- 鍵の一様な位置は、型検査が計算した一様な位置 (下の「一様な位置と制約付きの多相再帰」) を読んで `Flexible` にする。生成する関数の鍵の位置は、instance の節点の一様な位置に従う
- 解決に `Flexible` が要る位置は、どれも型検査の誤りになる。操作ごとの型変数は与えられた制約を持てないので E2006 に、決まらない推論用の変数は E2009 に、一様な位置の制約は E2012 になる。そのため translate は、制約付きの参照の解決で `Flexible` に出会わない。出会えば内部の誤りとして止める
- 変換の入口には、制約を持つ関数を選べない。入口は変換の呼び出し側が選び、制約を持つ関数を渡すのは呼び出し側の誤りとして `assert` で止める。`main` は制約を持たない
- メソッドの呼び出しの引数の数は、instance の等式の引数の数でなく、メソッドのシグネチャの矢印の数で決まる ([式](expressions.md) の「関数適用」)。instance のメソッドの等式がそれより少ない引数しか取らないときは、その関数を等式の引数の数で呼び、残りの引数を結果に `apply` する。引数のない instance のメソッドの直接の呼び出しは `mask` を持たず、残りの矢印の `mask` は `apply` が持つ

関数の名前は次のとおりである。

- instance のメソッドの関数の名前は、`<モジュールの修飾><クラス> <型>.<メソッド>` に、単相化の鍵の表示 (`@[…]`) を付けた形である (`"Prelude.Eq Option.==@[Int]"`、入口のモジュールなら `"Show Color.show"`)。修飾は instance のモジュールの規則 (上の「変換の規則」) に従う。クラスと型は、HIR が付けた名前 (診断と同じ表示) で書く。REPL のセッションの instance のメソッドだけは、上の「変換の規則」の名前のための表で書く。診断の表示名は型を定義し直すと古い版に `#N` を付けるので、古い instance のメソッドの名前が変わり、新しい instance のメソッドが古い名前を受け継ぐためである。導出とタプルの生成する関数の名前も、名前のための表で書く。空白を含むので、テキストの形では引用符で囲む
- 既定のメソッドの関数の名前は、メソッドの正式な名前に鍵を付けた形である (`"Prelude.!=@[Color]"`)
- 生成した関数の名前は、手で書いた instance のメソッドと同じ形にする。型の名前は、タプルなら配置の名前 (`(,)`)、`Unit` なら `()` である (`"Prelude.Eq (,).==@[Int, String]"`、`"Prelude.Show ().show"`)。修飾は、導出した instance なら型を定義したモジュール、タプルと `Unit` なら `Prelude.` である
- 補助の関数 `tag$<型>` は、`$con$` と同じ修飾の規則に従う (`tag$Option`、`tag$Report.Csv.Row`)
- 生成した関数と `tag$` は `internal` にしない。多くの関数から呼ばれるためである
- instance の `extern` の行の名前 (`Prelude.Eq Int.==`) も空白を含むので、テキストの形の `extern` の位置でも、関数の名前と同じ規則で引用符で囲む (`extern "Prelude.Eq Int.=="(a.0, b.1)`)

### 導出とタプルの生成器

translate は、導出した instance とタプルと `Unit` の instance の中心のメソッドを、(クラス, 型, 型引数) ごとに Core IR の関数として作る。フィールドの値は、`switch` の case の束縛か `unpack` で受ける (R8: `unpack` はコンストラクタが1つの配置にだけ使える)。フィールドの型のメソッドは、ほかの参照と同じく上の「メソッドの解決」で解いた行き先を呼ぶ。意味は [宣言](declarations.md) の「`deriving`」にある。

- 生成するのは中心のメソッドだけである。`Eq` は `==`、`Ord` は `compare`、`Show` は `show_prec` と `show` (`show x = show_prec 0 x`) を作る。`!=`、`<`、`<=`、`>`、`>=`、`display` はクラスの既定のメソッドを通る
- `==`: 左の値で `switch` し、コンストラクタ Ci の case で右の値を `switch` する。右の `switch` は Ci の case と `default` を持つ。Ci の case では、両方のフィールドの `==` を左から短絡して呼ぶ。`default` は `False` を返す。コンストラクタが1つの型 (タプルを含む) は、`switch` せずに両方を `unpack` する
- `compare`: 左の値で `switch` し、Ci の case で右の値を `switch` する。右の `switch` の Ci の case では、フィールドを左から `compare` し、最初に `EQ` でない結果を返す (すべて `EQ` なら `EQ`)。`default` では、補助の関数 `tag$<型>` で両方のタグの番号を `Int` で求め、`Int` の `compare` の結果を返す。`tag$<型>` は型ごとに1つ作り、`switch` 1つと、定数を返す case からなる。コンストラクタの組ごとに case を作ると大きさが `n²` になるので、この形にする。コンストラクタが1つの型は、`unpack` してフィールドを比べるだけである
- `show_prec d x`: `x` で `switch` し、[宣言](declarations.md) の「`deriving`」の形の文字列を `++` でつないで作る。`d` との比べ方で括弧を付けるかを決める
- タプルは、コンストラクタが1つの配置として同じ規則を通る。`Unit` の `==` は `True`、`compare` は `EQ`、`show_prec` は `"()"` を返す
- 型がすべて `Unr` なので、写しと解放は Perceus に任せる。生成した関数は、translate の出力として verifier を通る
- 生成した関数の中の `extern` 命令は位置を持たない。使う行はどれも純粋で、実行時エラーを起こさない
- 生成した関数は、HIR の関数の instance の後に、見つけた順に並べる

### 表示関数

REPL の式の入力の値は、translate が入力ごとに組む表示関数 (`translate/display.rs`) が文字列にする。書き方と上限は [REPL の端末](repl.md) の「値の書き方」と「上限」にある。

```
display$N$k (acc: String, budget: Int, depth: Int, prec: Int, x: T) -> (String, Int)
```

- 書き方の決め方: 部分の型 `T` ごとに、次の順で部分の種類 `Part` を決める
  - 関数型は `Function` (`<fn>`) である
  - `Show` を持たない extern の型は `Extern` (`<extern>`) である。中身を読めないためである。`FileIn` の値は式の入力の結果に現れない (E2016)
  - `eml_types::satisfies(Show, T)` が偽なら、構造で書く (`Display`)。このとき instance を解決しない。文脈が型引数で解けないので、`resolve` が返す instance は `method_target` で使えないためである
  - `eml_types::resolve(Show, T)` が、タプル (`Resolution::Tuple`)、導出した instance (`InstanceOrigin::Derived`)、頭が `LangItems` の `list` の instance のどれかなら、構造で書く
  - ほかは `Show` で、`show_prec` の行き先を `Collector::method_target` で解決する。入力の値そのものと `List` の要素では、`show` の行き先を解決する
- 形: `acc` に書いた結果を後ろに足し、残りの予算と組にして返す。Prelude の `show_rest` と同じく、左辺を一意な文字列として伸ばすので、書く量に比例する時間で済む ([オブジェクトモデル](object-model.md) の「文字列の操作」)。括弧も `acc` に先に足すので、入れ子の括弧で写しが増えない。結果の組 `(String, Int)` は2要素のタプルの配置 (`obj` で、フィールドは `tobj`) で返し、呼び出し側は `unpack` で受ける。組を解放した後の文字列は一意なので、次の連結もその場で伸ばせる
- 関数の始めで、`budget` が 0 なら `...` を足して返す (予算は 0 のまま)。`depth` が 100 以上なら `...` を足し、予算を1つ使って返す。どちらでもなければ予算を1つ使って値を書く
- 部分は、それを持つ値より1段深い (`depth + 1`) ところで、上の予算と深さの検査を受ける。`Show`、`Function`、`Extern`、`Ellipsis` (下の段と数の上限で表示関数を作らない型。`...` と書く) の部分は関数を作らず、呼び出し側でその場に組み、検査もその場で行う。`Display(k)` の部分は、表示関数 `display$N$k` を `depth + 1` で呼ぶ。`Show` の部分は、行き先 (`show_prec prec x`、`List` の要素なら `show x`) の結果を `acc` に足す。方程式の引数がメソッドより少ない instance のメソッドは、導出の生成器の `call_target` と同じく、返った関数に残りを適用する
- タプル、`data` のコンストラクタ、レコードは、導出した `Show` と同じ優先度と括弧で組む ([宣言](declarations.md) の「`deriving`」)。値の形は、導出の生成器と同じく型とフィールドの型から作る (`derive.rs` の `shape`)。`List` は、要素をたどる補助の関数 `display$N$k$rest(acc, budget, child, xs)` で背骨をたどる。要素は `depth + 1` (`child`) で書き、要素の前に予算が尽きていれば `...` を書いて閉じる。補助の関数は自分を末尾で呼ぶので、縮約が末尾呼び出しにする
- 名前: 入力の番号 N と、入力の中で見つけた順の番号 k から作る。型から名前を作らない理由は2つある
  - 後の入力が `instance Show Box` を足すと、前からある型 (`(Box, Int -> Int)`) の書き方が変わる。名前を型だけで決めると、同じ名前で違うコードができ、「同じ名前の関数はいつも同じコードになる」規則 (上の「変換の規則」) が破れる。入力の番号は使い回さないので、表示関数はその入力にだけある
  - 入れ子の型 (`data Nest a = | NNil | NCons a (Nest (a, a))`) では、たどる型の表示が段ごとに倍に伸びる。型の表示から名前を作ると、名前を作るのに指数の時間がかかる
- 集め方: instance の集め手 (`instances.rs` の `Collector`) が、入口の型から幅優先で型をたどり、型ごとに入口からの最小の段を記録する。表示関数の型は入口の型か別の表示関数の型からだけ見つかるので、幅優先で初めて見た段が最小の段である。番号 k は幅優先で見つけた順である。部分の `show_prec` と `show` の行き先も同じ集め手が集める
  - 初めて見た段が 100 以上の型は、表示関数を作らずに `Ellipsis` にする。その型の値を書く時点の段はいつも 100 以上で、`...` しか書かないためである
  - 1つの入力で作る表示関数は 1000 個までにする。幅優先で 1000 個を超えて見つけた型は `Ellipsis` にする。入れ子のフィールドを2つ持つ型 (`data W a = | WNil | W a (W (a, Int)) (W (a, Bool))`) は段ごとに型の数が倍になり、段の上限だけでは 2^100 個の型をたどるためである。lowering は止める語を読む前に走るので、ここで止まらないと Ctrl-C でも抜けられない
- 表示関数は `Found::Display` として、導出した instance の関数の後に、見つけた順に並ぶ。`internal` でない (導出の生成器と同じ)。互いに呼び合い、値としては使わない。`yields` でない。変数の印はすべて `unr` である。表示する値はいつも `Unr` で (式の入力の結果が `Unr` でなければ E2016)、`Unr` の値の部分も `Unr` だからである。写しと解放は Perceus に任せる。中の `extern` 命令 (比較、足し算、引き算、連結) は位置を持たない。予算と段は小さい数なので、溢れない

### 一様な位置と制約付きの多相再帰

一様な位置のグラフ (上の「変換の規則」) は、型検査 (`eml_types`) が型検査の後に、書き出した本体の表から計算する。translate は結果 (`TypedProgram::uniform`) を読むだけである。型クラスのために、節点と辺を次のように広げる。

節点は次の3種類である。

- (関数, 型変数の番号): 関数には、instance のメソッドの関数と既定のメソッドの関数を含む
- (instance, 頭の型変数の番号): 手で書いた instance、導出した instance、プログラムに現れる大きさのタプルの instance である。以下、instance の節点と呼ぶ。タプルの節点は、大きさごとに `Eq`、`Ord`、`Show` で1つを共有する。`Unit` は位置を持たない
- (メソッド, メソッド自身の型変数の番号): 以下、メソッドの節点と呼ぶ

辺は、本体の関数 f の中の参照ごとに引く。「f の型変数 i が型 T に現れるとき、(f, i) から X へ辺を引く」の形で書き、T が i の型変数そのものでなければ大きくなる辺とする。「現れる」は関数の辺と同じである。

1. 関数の参照 `g @[T0, …]`: Tj について (g, j) へ辺を引く (上の「変換の規則」と同じ)
2. メソッドの参照 `m @[T, U1, …, Uk]` の `T` の頭で instance I が決まるとき (型コンストラクタの適用かタプル): 解決先の関数へ直接辺を引く
   - 解決先が I のメソッドの関数 F (頭の型変数が n 個) なら、`T` の型引数 Tj について (F, j) へ、Ul について (F, n + l) へ引く
   - 解決先が既定のメソッド D なら、`T` について (D, 0) へ、Ul について (D, l) へ引く。さらに、根の制約 `C T` も 4. の規則で解決の木として扱い、(I, j)、I の文脈の部分木、頭の型の上位クラスの instance へ辺を引く。D の本体は、与えられた制約 `C a` を通して I のほかのメソッドを呼ぶので、D への辺だけではその流れがグラフから漏れるためである。たとえば、`same (Box x) (Box y) = differ (Box (Box x)) (Box (Box y))` が既定の `differ x y = not (same x y)` を呼ぶと、単相化は `same@[T]`、`differ@[Box (Box T)]`、`same@[Box T]` と止まらない。この辺は保守的で、クラスの制約を使わない既定のメソッド (`differ x y = True`) でも、同じ形を E2012 にする
   - 解決先が生成する関数 (導出とタプル) なら、`T` の型引数 Tj について (I, j) へ引く
3. メソッドの参照の `T` が与えられた制約で解けるとき: `T` については辺を引かない。Ul について (m, l) へ辺を引く
4. 制約の解決: 本体の中の参照が求める制約 (関数の参照の制約と、メソッドの参照のメソッド自身の制約) を、[型と Kind](types.md) の「制約の解決」と同じ規則で解く。メソッドの参照の `C T` は、解決の木の根を 2. が扱い、根の instance の文脈から下の部分木だけをここで扱う。解決の木の、instance で解いた各節点 `D (H T1 … Tn)` について、Tj について (D の instance, j) へ辺を引く。各節点で、`D` の上位クラスの (H の) instance にも同じ辺を引き、その instance の文脈の解決の木にも同じ規則を当てはめる。与えられた制約で解けた部分には辺を引かない。その証拠は、呼び出し側の解決の辺が表す
5. instance の節点から: (I, j) から I のメソッドの関数の (F, j) へ、大きくならない辺を引く。逆向きの辺は引かない。メソッドの関数は頭の型を作り出さないので、逆向きの流れはないためである。I がメソッド m を省いて既定のメソッド D を使うときは、(I, j) から (D, 0) へ大きくなる辺を引く (既定のメソッドのクラスの型変数には頭の型そのものが入るため)。導出した instance とタプルの instance は、生成するコードが求める制約 (フィールドの型 `F` の `C F`) を、本体を (I, ·) とみなして 4. の規則で扱い、使う既定のメソッドへの辺を同じく引く
6. メソッドの節点から: (m, l) から、m を定義するすべての instance のメソッドの関数の (F, n_F + l) と、m の既定のメソッドの (D, l) へ、大きくならない辺を引く

関数の参照の制約を解いた先 (4. と 5.) は、instance のすべてのメソッドへ辺を引くので保守的である。メソッドの参照は、2. で解決先にだけ辺を引くので、同じ instance の別のメソッドの間に偽の循環を作らない。ただし、既定のメソッドへの解決は、2. の追加の辺で instance の節点を通る。

大きくなる成分の節点を一様な位置とする。一様な位置と制約の関係は次のとおりである。

- 一様な位置の型変数が与えられた制約を持てば、E2012 にする。与えられた制約を持つ位置は、関数のシグネチャで制約を持つ型変数、instance のメソッドの関数で instance の文脈かメソッド自身の制約を持つ型変数、既定のメソッドのクラスの型変数とメソッド自身の制約を持つ型変数、instance の節点で文脈を持つ型変数である。証拠を単相化で確定するには、大きくなる型ごとに instance が要り、その数に限りがないためである
- 誤りは、関数ならシグネチャの名前を、instance のメソッドと instance の節点なら instance の頭を指す。既定のメソッドの位置なら、その位置を含む大きくなる成分へ 5. の辺 (既定のメソッドへの辺) を引いた instance の頭を指す。既定のメソッドは Prelude にあることが多く、そこを指しても直す場所が分からないためである。報告は大きくなる成分ごとに1つにまとめる。成分の中で与えられた制約を持つ位置 (instance の頭と関数など) のうち、プログラムの順 (モジュールの順、その中のソースの順) で最初の位置を主な位置にし、ほかの位置を副の位置として示す。どの位置も同じ循環で大きくなるので、位置ごとに報告すると同じ原因の誤りが重なるためである。ほかの成分で同じクラスについて示した位置は重ねて示さない。同じ instance の頭で、別の型変数が別のクラスの制約を持って別の成分で大きくなるときは、成分ごとに報告する。報告の順は、成分の最初の位置の順で、同じ位置なら成分の番号 (強連結成分を求めた順) の順である
- 入れ子のデータ型に `deriving Show` を付けた形 (`data Nested a = | Flat a | Nest (Nested (a, a)) deriving Show`) は、5. の辺の自己ループが大きくなる辺なので、E2012 になる

### 食い違いがないことの検査

V2 は、`lin` の値を `unr` の受け手に渡すことだけを拒む。`unr` の値を `lin` の受け手に渡すのは許し、`poly` はどちらの向きも通す ([LIR](lir.md) の「Kind の印の検査」)。印は型から決まるので、書き出した線形性か `kind_values` の評価が誤ると、両側が同じに誤った印を付けた受け渡しは V2 で見つけられない。`unr` から `lin` への受け渡しは V2 が見ないので、`lin` を持つ値に `unr` の印が付く誤りは、値を作る所の検査 (`check_built`) が止める。そこで translate は、debug ビルドで次の3つを確かめ、破れていれば内部エラーで止まる。release ビルドでは確かめない。

2つの型の比べ方は `TypeStore::lin_compatible` である。2つの型を構造をそろえてたどり、対応する線形性 (矢印の `lin`、`Flexible` と `OpVar` の Kind) の一方が `Unr`、他方が `Lin` なら食い違いとする。`Poly` はどちらとも食い違わない。片方が `Flexible(k)` か `OpVar(_, k)` で他方が別の形の型 `T` なら、`k` と `linearity(T)` を比べる。`Rigid` はどの型とも合う。形が一致しない所 (誤りの跡) は比べない。

- 参照の検査: instance を集めた後に、各関数の instance の中の参照 (関数を指す式) ごとに、代入した参照の型と、呼ばれる instance の型を比べる。比べ方は、参照の一番外側の矢印を飛ばす `lin_compatible_inner` である。その矢印は参照ごとの変数で、宣言の矢印と比べる相手ではないためである ([型と Kind](types.md) の「値を読む式の矢印の線形性」)。飽和した呼び出しだけでなく、部分適用と値としての関数の参照も入る。導出とタプルの生成器の instance と extern の関数は、比べる型を持たないので比べない。extern は行の印の列で確かめる (`FunctionRow::param_marks` と `ret_mark`。`std/` のシグネチャとの結び付けは `crates/eml_core_ir/tests/externs.rs`)
- 操作の検査: 各 `perform` の参照の型を、その参照の具体化の `kind_values` で評価した操作の宣言の型と、`lin_compatible_inner` で比べる。参照の一番外側の矢印は、参照の検査と同じく参照ごとの変数なので比べない。handler の節の引数のパターンの型と `k` の型は、操作の宣言の型と、`Rigid` をどの型とも合う型、`Var` を `Poly` として比べる。節の具体化は記録されていないので、`kind_values` は求めない。この比べ方で確かめられるのは、引数の型の矢印と、結果の型の中で呼ぶ側から節へ渡る位置の矢印が、節の側でも宣言の `Unr` と食い違わないことである
- 捕獲の検査: 両側が同じに誤った印 (`Lin` の値を持つ `unr` のクロージャ) は、比べ方では見つけられない。そこでクロージャを作る所で、矢印が `Unr` なら、捕まえた値の型の `linearity` がすべて `Unr` であることを確かめる

捕獲の検査を行うのは、translate がクロージャを作る所のうち、ラムダと、知っている関数と構成子の部分適用 (`closure` を作る飽和しない呼び出し) である。次の所は確かめない。状態のある `k v` (クロージャでなく `apply` になる)、値として渡された関数の実行時の部分適用 (呼ばれる側が分からない)、操作の節のクロージャ、handle の本体のクロージャ、継続を包む関数である。これらの矢印の線形性は、操作と継続の宣言から決まり、印でまとめて扱う (上の「Kind の印」)。

### 位置

- `extern` 命令は、呼ばれる側の位置 (`Loc`) を持てる。位置は、呼ばれる側の範囲の先頭 (演算子のトークンか extern の名前) である。値として使う extern は、参照した場所の位置を包む関数の `extern` 命令が持つ
- `Loc` は、ファイル、行、列を持つ。ファイルは `Program.files` (表示用のパスの表) の添字で、行と列は1から数える。列は行の中の文字の位置である
- 位置を持つのは `extern` 命令だけである。実行時エラーの表示に使う ([LIR](lir.md) の「実行時エラー」)

### REPL の入力

REPL の入力から作る関数 `input$N` (N は入力の番号) は、`()` を1つ受ける関数で、`main` と同じく `entry$input$N` から `()` で呼ばれる ([VM](vm.md) の「追記」)。入力の関数は普通の関数のまま入力の値を返し、入力の種類ごとの扱いは `entry$input$N` に置く。

- 式の入力の `entry$input$N` は、型検査が記録した表示の仕方 ([型と Kind](types.md) の「REPL の入力」) に従う。`Unit` なら、結果をそのまま返す。`Value` と `Typed` なら、`println` を呼ばずに、値を書いた文字列 (型の ` : T` を含まない) を返す。`ret` は `obj` である。`Repl` がその文字列を整形し、`Typed` なら ` : T` を付けて書く ([REPL の端末](repl.md) の「結果の表示」)
  - 入力の値の型を構造で書くなら、表示関数 `display$N$0("", 300, 0, 0, v)` を呼び、結果の組の文字列を返す (下の「表示関数」)
  - `show_prec` で書く型なら、`show` を呼んだ結果を返す。`show` は補間の穴と同じく `LangItems` (`show_method`) で引くので、利用者が同じ名前を定義しても変わらない
  - 関数型なら、入力の関数を呼んでから値を捨て、`<fn>` の定数を返す。式の中の `println` などは動く
  - `Show` を持たない extern の型なら、入力の関数を呼んでから値を捨て、`<extern>` の定数を返す
  - 入口は位置を持たない。書けなかったときの誤りの位置 (入力の式の先頭) は、`Repl` が HIR の入力の関数の本体から作る
- `let x = e` の入力の `entry$input$N` は、結果を `prim root_set[N]` で根に置く。根の番号は入力の番号である ([オブジェクトモデル](object-model.md) の「セッションの根」)
- 前の入力の `let` の束縛への参照は、どこにあっても `prim root_get[i]()` になる。関数の定義の本体からも参照できる。値の Repr と根の `tobj` の変換は、box の挿入が入れる
- `:type` の問い合わせの入力の関数 (`InputKind::Query`) は translate に渡らない。`Repl` が lowering しないためで、渡れば内部の誤りである
- 変換の段の verifier は、translate が出す基本操作として `root_get` と `root_set` と、`var` の節の `empty_mut_array` だけを受ける (下の「verifier」)
- セッションの item は、種類 (型、コンストラクタ、エフェクト、操作、クラス、メソッド、instance、関数) によらず中段の名前を持つ。前の入力が同じ名前の item を (名前空間によらず) 定義していなければ名前そのもの、定義していれば定義した入力の番号を付けた `名前#N` である。前の入力は変わらないので、前の版の名前は後の入力で変わらない。名前空間の違う同じ名前の item (型とそのコンストラクタ) は IR の別の表に入るので重ならない。エフェクトの名前、`$op$E.op`、`$con$A`、配置の名前、`mask$[E]` も中段の名前から作る。セッションのモジュールの名前 (`<input N>`) は接頭辞にしない
- `#` を含む関数の名前は、テキストの形では空白を含む名前と同じく引用符で囲む ([テスト戦略](../implementation/testing.md) の「LIR のテキストの形」)。エフェクトと配置の名前は、`#` を含んでも1つの語として読めるので囲まない
- `$op$`、`$con$`、`$cont$`、`$cont$state` は複数の定義が値として使う `internal` の関数である。どれも値と部分適用でだけ使われ、直接は呼ばれない。ABI は名前の型引数で決まる。`optimize` もこれらを直接呼ばない。引数のそろった `apply` は包んでいる命令に戻し、これらを複製しない (下の「最適化」の「包む関数」)。どのプログラムでも同じコードになるので、REPL の追記と食い違わない。直接は呼ばれないことを、テストで固定する
- `optimize` の複製の名前は、呼び出し先の instance の名前と、閉包の関数の名前と捕獲の数から作る (下の「最適化」の「鍵と名前」)。前の入力の高階関数に後の入力のラムダを渡すと、後の入力の関数の名前を含む複製ができる。名前が同じ複製はどの入力でも同じコードになるので、追記の確かめを通る。handler の複製 `<f>{H=<ret>}`、継続の関数 `<f{H}>$h<N>`、節の写し `<g>{k}` の名前も、経路の関数と handle の節の関数の名前だけから決まる (下の「最適化」の「handler の複製」)
- 構造で書く型を表示する入力のたびに、その入力の表示関数が `CodeBase` に増える。`entry$input$N` が入力ごとに増えるのと同じで、受け入れる

## パス

型付き HIR から LIR までを、次のパスの順に作る。Core IR のパスは translate、`optimize`、EP で、その後に LIR への変換と LIR のパス ([LIR](lir.md) の「パスの順番」) が続く。

| パス | 受け取る IR | 渡す IR |
|---|---|---|
| 変換 (`translate`) | 誤りのない型付き HIR | Core IR。基本操作と `clause` はない |
| 最適化 (`optimize`) | translate の出力 | translate の出力と同じ段の Core IR。既知の閉包への `apply` を直接の呼び出しにし、既知の閉包を受ける関数と閉包を複製にし、本体が既知の `handle` を handler の複製の呼び出しにし、小さな関数を展開して、既知のコンストラクタを解き、既知の閉包をフィールドに持つ `con` を受ける関数を複製にしたもの (下の「最適化」) |
| evidence passing (`evidence`) | `optimize` の出力 | `handle`、`perform`、`resume`、`mask` のない Core IR。基本操作、`clause`、EP が作る関数を含む |
| LIR への変換 (`to_lir`) | EP の後の Core IR | 変換の直後の LIR (下の「Core IR から LIR への変換」) |

- `eml_core_ir::Pass` は `Translate`、`Optimize`、`Evidence` の3つである。`core_until(input, Pass)` は、指定したパスの直後の Core IR で止める。`input` は `Input { hir, typed, entry, files }` で、どの入口も同じものを受け取る。テストは各パスの直後の IR を見られる
- 全体の入口は `lower(input)` と `lower_until(input, eml_lir::Pass)` で、どちらも LIR を返す。translate、`optimize`、EP、`to_lir`、`eml_lir::passes_until` の順に呼び、`lower` は reuse まで進む。2つの層をつなぐ順番を知るのは、この2つの関数だけである
- verifier はデバッグビルドだけでかけ、誤りはパスの名前を付けて報告する。translate と `optimize` の後は変換の段を、EP の後は EP の段をかけ (下の「verifier」)、誤りを `internal error: invalid Core IR after {pass}: {error}` と報告する。`to_lir` の直後には別の検査を置かない。`to_lir` の出力は EP の段の検査を通った IR を写しただけで、`passes_until` の入口の検査がすぐに同じことを確かめるためである
- どのパスの後でも、ブロックの列は構造の規則を満たす。パスの間で古くなる索引やキャッシュは IR に持たせない
- 後のパスの置き場所: 呼び出しを作るか行き先を変えるパス (インライン化、evidence passing など) は、Core IR の上で translate と box の挿入の間に置く。Core IR は `tail`、`box`、`unbox` を持たないので、こうしたパスはそれらを出さない。box の挿入がプログラム全体の ABI と変換を1か所で決め、縮約が末尾呼び出しを作るためである
- 再帰の深さ: 2つの IR のパス、verifier、テキストの表示と読み込み、VM のコード生成と実行、`Drop`、`Clone`、`Debug` のどれも、プログラムの大きさに比例して Rust のスタックを使わない。残るのは、変換が HIR の式の入れ子をたどる分 (E0013 が抑える) と、決定木のパターンの大きさの分 ([実装の現在地](../implementation/status.md) の「コンパイルの時間と深さ」) だけである

### 最適化

`optimize` (`optimize/`) は、translate の出力を EP の前に書き換える。EP の前に置くのは、EP が見る `apply` と `yields` の印を先に減らすためと、handler への特殊化が `handle` と `perform` の残る段で動くためである。

- 出力は translate の出力と同じ段の Core IR で、`handle`、`perform`、`resume`、`mask` を残す。基本操作、`clause`、`tail`、`box`、`unbox`、RC の命令は出さない (上の「パス」の後のパスの置き場所)
- 既知の閉包への特殊化は、関数ごとの作業リストで動く。translate の関数を順に書き換え、途中で作った複製を後ろに足して、同じ規則で書き換える。複製は鍵ごとに1つなので、作業リストは止まる
- 既知の閉包への特殊化 (`specialize`)、handler への特殊化 (`handlers`)、融合 (`fuse`) の順に動く。それぞれの後に `yields` の印を付け直し (下の「`yields` の付け直し」)、最後に入口から届かない関数を落とす。ランタイムが名前で呼ぶ関数は入口と並べて根にし、何も呼ばなくても残す。残った関数は元の順のまま番号を振り直す。

#### 既知の閉包

同じ関数の中で `closure f(c_1, .., c_n)` が束縛した変数と、関数の値 `&f` を、既知の閉包とする。複製の中で、閉包の捕獲に開いた引数の組も既知の閉包である (下の「複製」)。ブロック引数を通って合流する値と、データに入れてから取り出した値は、既知としない。

- 既知の閉包への `apply` は、`call f(c_1, .., c_n, 引数…)` にする。引数が足りなければ `closure` にし、部分適用はエフェクトを起こさないので `mask` を落とす。引数が余れば、結果の関数の値に残りを `apply` する。余る形を書き換えるのは `mask` が空のときだけで、`mask` の掛かる範囲を変えないためである。translate が呼ぶ相手の分かっている呼び出しにする場合分け (上の「変換の規則」) と同じである
- `unr` でない引数を、箱を要する Repr (`int`、`float`、`enum` と構造体) の引数で受ける閉包の関数には、直接の呼び出しにしない。構造体では保守的な規則である。一様な instance (`_:poly`) の中の値は型変数の印 (`poly`) のままである。box の挿入はこの値を `unbox` で読むだけなので、Perceus は値かその持ち主を `decref` で手放すことになり、V1 ([LIR](lir.md) の「Kind の印の検査」) に反する。この `apply` は残し、閉包は値として渡す
- `unr` でない既知の閉包は、使う所がすべて書き換えられるときだけ書き換える。書き換えない使う所が残ると、捕獲した値を、閉包と直接の呼び出しの2か所で消費するためである。使う所ごとの判断は、その閉包と使う所だけで決まるので、先に決めた答えと使う所での書き換えは食い違わない
- 書き換えで使う所がなくなった `closure` は消す。残すと、`unr` でない捕獲を2か所で消費する IR になる

#### 包む関数

translate の包む関数 (`$op$`、`$con$`、`$cont$`、`$cont$state`) は、名前でなく形で見分ける。引数をそのままの順で1つの命令に渡し、結果を返すだけの `internal` の関数である。`internal` に限るのは、トップレベルの関数の呼び出しを命令に開くのがインライン化の受け持ちだからである。

- 引数のそろった `apply` と直接の呼び出しを、包んでいる命令に戻す。`$op$E.op` は `perform E.op(引数…)` に、`$con$C` は `con C(引数…)` に、`$cont$` と `$cont$state` は捕獲の生の継続への `resume` にする
- `mask` の付いた `$op$` への `apply` は書き換えない。`perform` は `mask` を持てないためである
- 包む関数は直接呼ばない。複製の呼び出し先にもしない (上の「REPL の入力」)
- 包む関数は鍵の閉包の関数にしてよい。複製の中では、引数のそろった `apply` が包んでいる命令に戻り、包む関数そのものは呼ばれないためである。包むコンストラクタの結果は、結果の印が引数の印の合併以上のときだけ `con` に戻す。型変数の位置の `poly` の引数を `unr` の結果に入れる形は、組み立ての検査を通らないので `apply` のまま残す

#### 複製

既知の閉包を関数 g の引数に渡す直接の呼び出しは、特殊化した g の複製の呼び出しにする。複製では、その引数を捕獲の値の引数 n 個に置き換える。既知の閉包を別の閉包 `closure h(..)` の捕獲に入れるときも、同じく h の複製の `closure` にする。`Iter.map f it` が作る閉包の中身や、`Iter.generate` の handle の本体に特殊化を届けるためである。`handle` の本体、節、`return` の節も、関数の値の位置として同じく扱う。

- 特殊化する位置は、呼び出し先が本体でその引数を `apply` するか、包む関数でない関数の直接の呼び出しか `closure` に渡すか、`handle` に渡す位置に限る。データに入れるだけの引数を特殊化しても、閉包を作り直すだけで何も減らないためである
- 呼び出し先は translate の関数に限る。呼び出し先がすでに複製である呼び出しは、それ以上特殊化しない
- 複製の中の再帰 (`map f rest` のように、受け取った閉包をそのまま渡す呼び出し) は同じ鍵になるので、同じ複製を呼ぶ
- 複製の中で閉包そのものが値として要る位置 (データに入れる、`return` する、相手の分からない `apply` の引数にする、特殊化しない呼び出しに渡す、など) では、その場で `closure f(..)` を作り直す。捕獲のない閉包は `&f` にする
- 捕獲がなくなった複製の閉包は、関数の値 `&copy` にする

#### 鍵と名前

複製の鍵は (呼び出し先、特殊化する引数の位置ごとの閉包の関数と、捕獲の Repr と印) である。捕獲の値は鍵に入れず、引数として渡す。鍵が同じなら同じ複製を使い回す。

- 捕獲の Repr と印は、捕獲した値でなく、閉包の関数の捕獲の位置の引数からとる。そのため、捕獲の形は閉包の関数と捕獲の数で決まる。値の Repr と引数の Repr の違いは、呼び出しの所で box の挿入が1回だけ直す
- 開いた引数の印は、閉包の関数の引数の印と、呼び出し先のもとの引数の印のうち、小さい方 (`unr` < `poly` < `lin`) にする。開いた引数の印がもとの引数の印を超えないので、開いた引数を捕まえて作り直す閉包も、組み立ての検査を通る。たとえば `Yield.for_each` は生産者を `poly` で受けるので、`FileIn` を捕まえた生産者を開いた引数も `poly` になる
- 名前は、呼び出し先の instance の名前の後ろに、引数の位置と閉包の関数を書く (`"Std.List.map@[Int, Int]{0=main$lambda0}"`)。捕獲のある閉包は、関数の後ろに捕獲の数を書く (`{0=main$lambda0/1}`)。同じ関数の部分適用でも、捕獲の数が違えば複製の引数が違うためである。64字を超えた名前は、呼び出し先の名前の、鍵の `{…}` の外にある最初の `@` より前の部分に、捕獲の数を含む名前全体の 64 ビットのハッシュを付けて `<base>@#<16 hex>` にする。鍵の中の `@` で切ると、`{` の閉じない名前になるためである。名前は `{` と `=` を含むので、テキストの形ではいつも引用符で囲む ([テスト戦略](../implementation/testing.md) の「LIR のテキストの形」)
- 打ち切り: 特殊化で生まれた閉包をまた鍵にする連鎖 (CPS 風に閉包が育つ形) は、鍵の入れ子の深さで打ち切る。translate の関数の深さは 0 で、複製の深さは、鍵の閉包の関数の深さの最大に1を足した値である。引数の位置ごとに、閉包の関数の深さに1を足した値が `MAX_KEY_DEPTH` (4) を超える位置だけを特殊化せず、閉包を作り直して渡す。位置ごとに決めるので、ある位置で閉包を開くかどうかは、その閉包と使う所だけで決まる。`unr` でない閉包の判断 (上の「既知の閉包」) が、呼び出しのほかの引数に依らないようにするためである
- 決定性: 複製は呼び出し先の translate の出力から作り、そのあと同じ規則で書き換える。特殊化するか、どこで打ち切るかの判断は、鍵と translate のコードだけで決まり、プログラム全体の大きさの予算は使わない。作業リストの順でほかの関数をどこまで書き換えたかが変わっても、複製のコードは変わらない。こうして、名前が同じ関数はいつも同じコードになる ([VM](vm.md) の「追記」)
- 複製は呼び出し先の `internal` の印と一様な引数の数を写す。値として生まれた関数 (ラムダ、handle の本体、節と `return` の節) の複製は内部の関数で、ほかの関数の複製は内部の関数でない。ABI は型と `split` で決まり、複製がプログラムによって直接呼ばれても値として使われても変わらない。値の参照が型どおりの位置を指せば、box の挿入が版を作る ([LIR](lir.md) の「関数の ABI」)。もとの関数の印は変えない

#### handle を含む関数の複製

- `handle` を含む関数を複製するときは、その `handle` の節と `return` の節の関数も、複製の名前を持ち主として鍵に入れて一緒に複製する (`<callee>@{…} via <owner>`)。捕獲に既知の閉包がなくても複製する。EP は、節の関数がちょうど1つの `handle` に属することを前提にして、節の関数をその場で入口の関数に書き換えるためである (`evidence/handlers.rs` の `entries`)。同じ鍵の複製は1つしか作らないので、複製した節の関数も1つの `handle` に属する
- `handle` の本体の関数は、ほかの複製と同じ鍵で複製する。EP が1つの `handle` への所属を求めるのは節だけなので、本体の複製はいくつもの `handle` で共有してよい

#### handler への特殊化

本体が既知の関数になった `let x = handle E(init, body) { clauses } return ret` を、経路の関数の handler ごとの複製の呼び出しに替える。以下では、この handle を H と書く。`Iter.generate` と `Yield.fold` の handle の本体は、上の「複製」で既知になる。特殊化した handle は evidence を作らず、H 宛ての `perform` は yield しない。

- 特殊化する handle は、std の印でなく Core IR の形で見分ける。`Async` のスケジューラが、待っているタスクの `k` を返す形で書かれれば、同じ規則で特殊化できるようにするためである
- 既知の閉包への特殊化と1回目の `yields` の付け直しの後に動き、そのあともう一度印を付け直してから、届かない関数を落とす。下の「入れ子の handler」のとおり、回を重ねて動く。複製と継続の関数は、その回の初めのコードから作る。そのコードは名前ごとに決まっているので、どの handle を特殊化するかも、複製のコードも、handle を含む関数と経路の関数のコードだけで決まる ([VM](vm.md) の「追記」)
- 状態つきの handler は、`init` が `()` でない handler である。`from ()` の状態つきの handler は、状態のない handler と同じに扱う。状態の値が `()` なので、渡しても省いても同じである
- そのため、`from ()` の handler の、`k` を値として使う節 (`Iter k`) は特殊化しない。translate はその節で `k` を `$cont$state` で包むが、状態のない handler として扱うと包む関数が `$cont$` でないので、下の条件の最後に当たる

特殊化には2つの形がある。形は下の「handler の節の形」で決まるので、1つの handle はいつも1つの形になる。

- **状態を渡す形**: どの操作も再開する操作で、どの節もすぐに再開する節である handler の形。`Std.State`、`Yield.fold`、`Yield.for_each` がこの形になる。経路がすべて末尾の呼び出しでも、この形にする。1つの handler に2つの形を持たないためである
- **末尾の経路の形**: それ以外の handler の形。`never` の操作を持つ handler と、それ以外の節を持つ handler (`Iter.generate` など) が当たる

経路の関数は、handle の本体から、`yields` の関数の直接の呼び出しをたどって届く関数である。末尾の経路の形では末尾の位置の呼び出しだけをたどり、状態を渡す形では末尾の位置かどうかによらずたどる。末尾の位置は、EP と box の挿入と縮約が使うのと同じ定義 (`tail.rs` の `tail_position_call`) で決める。

- 状態を渡す形では、H に触れる関数への呼び出しだけをたどる。H に触れるとは、その関数か、直接の呼び出しでたどれる関数が、H 宛ての `perform`、`mask` の付いた呼び出し、`handle`、`resume`、`clause`、`yields` の `apply` のどれかを持つことである (`optimize/handlers/touch.rs`、直接の呼び出しについての最小の固定点)
- H に触れない関数は経路に入れず、複製の中でも呼び出しをそのまま残す。H の evidence を引かないので、H の handle を特殊化して evidence がなくなっても、そのまま動く。経路に入れると、H の `perform` に届かない関数も複製され、使わない状態を受け渡して組で返すことになる
- 末尾の経路の形の経路は H に触れるかどうかによらずたどる

次のどれかに当たる handle は特殊化せず、EP の道に残す。

- handle に `mask` が付いている
- 本体、節、`return` の節のどれかが、関数の値でも、handle を含む関数の中で `closure` が束縛し、この handle だけが使う変数でもない。捕獲を複製の引数に開くので、閉包をほかの所でも使うと、`unr` でない捕獲を2か所で消費することになるためである
- エフェクトの操作がすべて `never` である (`Std.Fail` と `IO`)。中断はすでに巻き戻しで安いためである
- 経路の関数に、`mask` の付いた呼び出し、入れ子の `handle`、`resume`、`yields` の `apply` (末尾の位置かどうかによらない) がある。末尾の経路の形では、末尾の位置でない `yields` の関数の直接の呼び出しも当たる。yield しうる操作を持つエフェクトの `perform` は、それを含む関数の印に必ず表れる ([型と Kind](types.md) の「row の具体化の記録」)。そのため、経路の外の `perform` が H に届かないことは、この印で安全側に判断できる
- 扱うエフェクトが `never` の操作を持ち、経路の関数が、`yields` でない関数のうち付け直す前の translate の印が立った関数を直接呼ぶ。付け直しは再開する操作の `perform` だけを yield の地点に数えるので、`never` の操作だけを起こす関数は印を失い、特殊化した後の `perform` が handler を見つけられなくなるためである。translate の印は名前ごとに決まるので、判断も名前ごとに決まる
- 経路の関数の H 宛ての `perform` の引数が、節の関数の引数と同じ Repr でないか、印を受け渡せない。節の本体を展開すると、box の挿入が直す呼び出しの境目がないまま、違う Repr の値を使うことになるためである。展開するすぐに再開する節では、引数の印が節の引数の印を超えないことも求める (`unr` < `poly` < `lin`)。展開した本体は引数を何度も使いうるので、`unr` の引数の所に `poly` の値を置くと、`unr` でない値を写すことになる ([LIR](lir.md) の「Kind の印の検査」の V1)。それ以外の節には呼び出しで渡すので、印は受け渡せれば足りる
- 状態つきの handler で、節の状態の引数の Repr と印が、`return` の節の状態の引数と違う。複製は受けた状態をそのまま節に渡すためである
- 下の「handler の節の形」の「それ以外の節」の `ret` の Repr が `return` の節の `ret` と違うか、印を受け渡せない。節の結果が、そのまま複製の結果になるためである
- 再開する操作の「それ以外の節」で、`k` が、`resume k(..)` の呼ばれる値、`k` を包む `closure $cont$(k)` か `closure $cont$state(k)` (handler の状態の有無と合う方)、`drop k` のほかに現れる。生の継続を入れ子の関数が捕獲する形は、その関数の中の `resume` も書き換えることになるので扱わない
- 状態を渡す形で、経路の関数の H 宛ての `perform` の結果の変数と、その操作の節の末尾の `resume k(v, s)` の v の Repr が互換でないか、印を受け渡せない。状態つきの handler では、s と `return` の節の状態の引数も同じく比べる。展開した節の `resume` は `perform` の後ろのブロックへの `jump` になり、`jump` の実引数には box の挿入が変換を入れないためである ([LIR](lir.md) の「位置の規則」)。v が操作の引数なら、置き換える `perform` の実引数で比べる。この条件に当たる handler は、経路がすべて末尾の呼び出しでも、末尾の経路の形に切り替えずに EP の道に残す。末尾の経路の形にすればこうした handler も特殊化できるが、1つの handler に2つの形を持たないために特殊化しない ([計測の基準](../implementation/benchmarks.md) の「状態を渡す形にしない handler」)

#### 入れ子の handler

入れ子の handler は内側から特殊化する。経路はどの `handle` も断るので、内側の handle が残る間は、それを経路に含む外側の handler を特殊化しない。内側の handle を特殊化すると、その handle は複製の呼び出しに変わって消え、外側の handler の経路は内側が作った複製を通ってたどれるようになる。そこで、handler への特殊化を回に分けて繰り返す。

- 1回ごとに、その回の初めのコードで上の条件を満たす handle をすべて特殊化する。複製と継続の関数は、その回の初めのコードから作る
- 外側の handler は内側の複製の複製を作る。名前は接尾辞を重ねた `<f>{H=<r1>}{H=<r2>}` である (下の「handler の複製」)
- 複製の表 ((f、`return` の節の関数) から複製への表と、節の関数から写し `<g>{k}` への表) は回をまたいで持つ。同じ名前の関数は一度だけ作る。経路の関数は `handle` を持たないので後の回で書き換わらず、前の回に作った複製はそのまま使える
- 回の間で `yields` の印を付け直す。経路は `yields` の印でたどるので、次の回の経路は、その回の書き換えの後の印で決める
- 回で作った関数は、もとの関数の translate の印を受け継ぐ。`f{H}`、`f{H}$hN`、`f{H}$hjJ` は f の印を、`<g>{k}` と `<g>{k}{…}` は g の印を受け継ぐ。上の条件のうち、translate の印を見る判断に使うためである
- 特殊化する handle のない回で止める
- 止まるのは、新しい handle が節の写し `<g>{k}` の中にだけ生まれ、その handle の入れ子の深さがもとの handle より小さいためである。経路の関数の複製と継続の関数は、経路が `handle` を断るので handle を持たない。`<g>{k}` の中の handle はもとの handle の節の中にあった handle で、その下に重なる handle の段の数は、もとの handle より少ない
- 経路に特殊化しない handle (`try_panic` のような、`never` の操作だけのエフェクトの handle など) が残る外側の handler は、特殊化しない

#### handler の節の形

節の関数 g を、Core IR の形から次の2つに分ける。`fn` と `ctl` の書き方には依らない。

- **すぐに再開する節**: 再開する操作の節で、本体に `handle` がなく、`return` で終わるブロックがどれも、末尾の位置の `mask` のない `resume k(v, s)` の結果を返し、`k` の出現がその `resume` だけである節。EP の「すぐに再開する節」(下の「`perform` と節の3つの形」) の4つ目の条件 (本体に yield の地点がない) は要らない。展開した節は handler の外側でふつうの関数の一部として動き、本体の yield は EP が扱うためである
- **それ以外の節**: `k` を値として使う節 (`Iter k`)、`k` を末尾でなく呼ぶ節、`k` を使わない節、`never` の操作の節、本体に `handle` を持つ節。`handle` を持つ節を展開しないのは、展開した所ごとにその `handle` が写り、節の関数がちょうど1つの `handle` に属するという EP の前提 (`evidence/handlers.rs` の `entries`) に反するためである。節の写し `<g>{k}` (下の「handler の複製」) は1つだけで、もとの g は使われなくなって消える。写しはその回の書き換える前の g から作るので、中の `handle` はその回には特殊化しない。上の条件を満たせば、次の回で特殊化する (上の「入れ子の handler」)

#### handler の複製

経路の関数 f を、handler ごとの複製 `f{H}` にする。

- 鍵は (f、`return` の節の関数) で、名前は `<f>{H=<ret>}` である。2つの形で同じ名前の書き方を使う。translate は handle ごとに `return` の節を作り、上の「handle を含む関数の複製」は持ち主の名前を鍵に入れて節を複製するので、`return` の節の関数が handle を1つに決める。節の関数と捕獲の形も、それで決まる。handle が特殊化の形を決めるので、名前が同じ複製はいつも同じ形で、ABI も同じになる。64字を超えた名前は、上の「鍵と名前」と同じく、f の名前の、鍵の `{…}` の外にある最初の `@` より前の部分に名前全体のハッシュを付けた `<base>@#<16 hex>` にする
- 後の回の複製は、内側の複製の名前を f の名前として、接尾辞を重ねる (`<f>{H=<r1>}{H=<r2>}`)。64字を超えると、上の規則どおり、内側の複製の名前の、鍵の `{…}` の外にある最初の `@` より前の部分が base になる。もとの f の名前に `{…}` の外の `@` がなければ、内側の複製の名前 `<f>{H=<r1>}` の全体が base になり、`<f>{H=<r1>}@#<16 hex>` の形になる。f が `Std.X.f@[Int]` のように型引数で名付けた関数なら、`Std.X.f@[Int]{H=<r1>}{H=<r2>}` の base は `Std.X.f` で、`Std.X.f@#<16 hex>` の形になる。f の名前に鍵の `{…}` の外の `@` があると (型引数で名付けた関数)、ハッシュの形の名前は `{H=` を含まないので、IR の表示では、名前から handler の複製と見分けられない

この後の、引数から `internal` の印までは、末尾の経路の形の複製の規則である。状態を渡す形の複製は、この節の終わりに書く。
- 引数は、f の引数、節と `return` の節の捕獲 (節の順で、`return` の節が最後)、状態つきなら状態の順である。状態の引数の Repr と印は、`return` の節の状態の引数のものである。`ret` は `return` の節の `ret` である
- handle の地点の `let x = handle …` は、`let x = call body{H}(本体の捕獲…, (), 節と return の節の捕獲…, init)` にする。状態のない handler では init を省く。handle のためだけに作った `closure` は消す
- `return r`: `return` の節を、その捕獲、r、今の状態で呼び、結果を返す。末尾の位置の、`yields` でない関数の直接の呼び出しと `yields` でない `apply` は、末尾でない呼び出しにして、その結果で `return` の節を呼ぶ
- 経路の関数への末尾呼び出し: 引数に節と `return` の節の捕獲と今の状態を足して、その関数の複製 `g{H}` を呼ぶ

H 宛ての `perform E.op(a…)` は、どれもそこで複製を終える。後ろから順に書き換えるので、前の `perform` の続きを切り出すときには、その中の後ろの `perform` がすでに書き換えてある。

- 再開する操作では、`perform` より後ろを継続の関数 `f{H}$hN` に切り出す。N は f の中の H 宛ての `perform` の、ブロックと文の順の通し番号である (0 から振る)。引数は、`perform` の後で生きている変数 (結果と状態を除き、番号の昇順)、再開の値 v、状態つきなら次の状態 s の順で、閉包として呼ばれるときは `$cont$` (`v` を受ける) と `$cont$state` (`v` と `s` を受ける) と同じ形である。`internal` にするかどうかは、下の `internal` の印の規則で決まる。`yields` の印は f の印を写し、後の付け直しで決まる。本体は `perform` の後ろのブロックの残りを入口にし、そこから届くブロックを元の順に続ける。変数の表は、下の EP の「継続の関数」と同じく、本体に現れる変数だけに詰める。接尾辞を EP の `$kN` と分けるのは、`f{H}` がほかのエフェクトで yield すると、EP が `f{H}$kN` を作って名前が重なるためである
- 合流のブロック: 継続の関数の本体は、f の合流のブロック J (入る辺が2本以上か、引数を持つブロック) の手前で止める。J への `jump` は、合流のブロックの関数 `f{H}$hjJ` を、`jump` の実引数、J の入口で生きているほかの変数 (状態を除き、番号の昇順)、状態つきなら今の状態の順で末尾で呼ぶ形にする。`f{H}$hjJ` の本体は J とそこから届くブロックで、継続の関数と同じく次の合流のブロックで止まり、中の `perform` も同じ規則で書き換えてある。J は f でのブロックの番号である。`internal` にせず、`yields` の印は f の印を写す。EP の継続の関数が合流のブロックで `f$jJ` を呼ぶ (下の「継続の関数」) のと同じく、コードの大きさを `perform` の数に比例させるためである。合流のブロックを写すと、条件つきの `perform` が並ぶ形で、合流のブロックとその先がその前のどの継続の関数にも入り、大きさが `perform` の数の2乗で育つ。展開した節のブロックは合流のブロックにしない。1つの `perform` の地点からだけ届くので1つの続きにしか入らず、節が状態を使った後なので、状態を渡す形にも合わないためである。接尾辞を EP の `$jJ` と分ける理由は `$hN` と同じである
- 生存: 複製のブロックを後ろから1回だけたどって求め、`perform` はたどる途中で書き換える。辺はすべて番号の大きいブロックへ向かうので、`perform` の後ろと合流のブロックの生存は、そこへ着いたときには書き換えた後のコードで決まっている。`perform` ごとに続きをたどり直すと、`perform` の数の2乗の手間になる
- すぐに再開する節: 節の本体を `perform` の地点に展開する。節の変数は新しい変数にし、捕獲と操作の引数を置き換える。末尾の `resume k(v, s)` は `f{H}$hN(生きている変数…, v, s)` の末尾呼び出しにする (状態のない handler では s を省く)。状態がいつも関数の引数なので、ブロックの間で状態を持ち回らず、組も確保しない。展開した節の文は、H 宛てとして書き換えない。節の中の `perform` は、もとの意味でも handler の外側で動くためである。g は再帰しないので、展開は止まる
- 定数の操作の引数: 展開した節の本体の `switch` が、引数を置き換えて定数 (`True`、引数のないコンストラクタ、整数) で分けるようになったら、合う case か `default` へのジャンプにする。定数で分ける `switch` は verifier が断るためである。飛ばした case のフィールドにも新しい変数を振り、辺の入らなくなったブロックは複製の最後に落とす
- それ以外の節: 本体に `handle` がなければ、節の写しをこの地点の継続の関数について複製した `<g>{k}{…}` (下の「節の写しの、継続についての複製」) を、節の捕獲、a…、生きている変数、今の状態で末尾で呼び、閉包は作らない。本体に `handle` がある節では、`let k = closure f{H}$hN(生きている変数…)` を作り (捕獲がなければ関数の値 `&f{H}$hN`)、g の写し `<g>{k}` を、節の捕獲、a…、k、今の状態で末尾で呼ぶ。閉包の印は g の `k` の引数の印で、`once` の操作なら `lin` である。`multi` の継続の捕獲は持ち越し規則 ([エフェクトと handler](effects.md)) で `Unr` に限られるので、`multi` の操作の閉包は `unr` の値だけを捕獲する
- `never` の操作: g を、節の捕獲、a…、今の状態で末尾で呼び、`perform` より後ろは捨てる。g は `k` を受けないので写さない
- それ以外の節と `never` の操作の2つの形では、末尾の呼び出しにも閉包にも渡さない `unr` でない値 (節と `return` の節の捕獲と、`perform` の後で生きていた変数) を、呼び出しの前に `drop` する。もとの意味でも、中断の巻き戻しと `prompt$` の `drop ret` が、そこで同じ値を捨てる。`unr` の値は Perceus が手放すが、`unr` でない値には `decref` を出せない ([LIR](lir.md) の「Kind の印の検査」の V1)

節の写し `<g>{k}` は、`k` に生の継続でなく継続の関数の閉包を受ける g の写しである。

- 鍵は g だけで、名前は `<g>{k}` である。g はちょうど1つの handle に属するので、状態の有無も g で決まる。64字を超えた名前はハッシュの形にする
- `k` の引数は位置を変えず、Repr を `tobj` にして印を保つ。`k` を包む `closure $cont$(k)` と `closure $cont$state(k)` は消して、使う所を `k` そのものにする。継続の関数の閉包は包む関数と同じ引数を受けるので、包む必要がないためである
- `resume k(v, s)` は、状態のない handler では `apply k(v)` に、状態つきでは `apply k(v, s)` にする。`apply` の `yields` の印は g の印にする。EP が `resume` から作る `apply` (上の「`apply` の印と分け方」) と同じである。`mask` はそのまま残す。この `apply` も、ほかの `apply` と同じく一様である。継続として渡す `f{H}$hN` は、生きている変数を捕獲として受け、その後の再開の値と状態と結果を一様にする (`split` は生きている変数の数)。値の参照が残らなければ、`optimize` の最後に `split` を外す (下の「直接だけ呼ばれる持ち上げた関数」)

`internal` の印は次のとおりである。

- 複製 `f{H}`、合流のブロックの関数 `f{H}$hjJ`、節の写し `<g>{k}` とその複製 `<g>{k}{…}` は、直接呼ばれるために生まれるので `internal` にしない
- 継続の関数 `f{H}$hN` は、`k` として節の写しに渡すもの (節が `k` を値として使う形) だけを `internal` にする。すぐに再開する節を展開した所で末尾で呼ぶだけの継続の関数は `internal` にしない。どちらになるかは、その `perform` の操作の節の形で決まり、節の形は名前で決まる
- handler への特殊化が作るほかの関数 (`f{H}`、直接呼ぶ `f{H}$hN`、`f{H}$hjJ`、`<g>{k}`、`<g>{k}{…}`) は、`split` を持たない。直接呼ばれるためである。値として参照されれば、box の挿入が版を作る。`internal` の印は ABI を決めない ([LIR](lir.md) の「関数の ABI」)。特殊化の後に直接呼ぶ `return` の節は `split` を持ったままなので、呼ぶ所に box の挿入が変換を入れる。`return` の節は小さいのでふつうは展開され、変換は覗き穴で消える

状態を渡す形の複製 `f{H}` (`optimize/handlers/passing.rs`) は、状態を変数で持ち回る1つの関数である。継続の関数と合流のブロックの関数は作らず、足す関数は複製だけである。テキストの形に新しい書き方はない。

- 複製は直接呼ばれるために生まれるので、`internal` にしない。`yields` の印は f の印を写し、後の付け直しで決まる
- 引数は、f の引数、操作の節の捕獲 (操作の順)、状態つきなら状態の順である。状態の引数の Repr と印は、`return` の節の状態の引数のものである。`return` の節の捕獲は受けない。複製は `return` の節を呼ばないので、受けると末尾でない呼び出しのたびに渡すことになり、`unr` でない捕獲を2か所で使うためである。操作の節の捕獲は `Unr` に限られるので (E3001、[線形性](linearity.md))、どの呼び出しにも、展開したどの節にも渡してよい
- 結果は値の並びで決まる。元の f が基の関数なら、元の並びは f の結果 (`ret` が `unit` でなければ。Repr と印は f の `ret` と `ret_mark`) である。f が前の回の状態を渡す形の複製なら、元の並びはその複製の並びである。末尾の経路の形の複製は `return` の節の結果を返すので、基の関数と同じに扱う。複製の並びは元の並びに、状態つきの handler なら状態 (Repr と印は `return` の節の状態の引数のもの) を1つ足したものである。並びは複製の元の名前と handler で決まるので、名前で中身が決まる規則は保たれる
- 並びの値がなければ `()` を返し、1つならその値をそのまま返す。2つ以上なら、その大きさのタプルを1つ作って返す。タプルの Repr は、値の Repr と印から作るタプルの形の構造体で (`Tuples`)、葉の語の数が 6 を超えるか、値がみな `unit` で 0 語なら `obj` である。印は値の印の結びである。`Unit` を返す関数 (`for` の本体など) が、呼ばれるたびに組を確保しないように、値のない並びを分ける。入れ子の handler でも、外側の handler ごとに組を重ねず、1つのタプルにまとめる
- handler が1つだけのときは、並びの大きさは、状態のない handler では f の結果の有無で0か1、状態つきでは1 (次の状態だけ) か2 (結果と次の状態の組) になる
- タプルは配置 `(,)`、`(,,)`、… の `con (,) #0(…)` で作る。構造体なら `con` は構造体の変数を束縛し、引数は形のフィールドの Repr と比べる正確な位置になる。`return` の値が結果の Repr と互換なだけで同じでなければ (`obj` の変数や `tobj` の位置の `()`)、値と状態を、並びの Repr の引数を持つブロックへの `jump` で渡し、そこでタプルを作る。`jump` の実引数は互換の位置で、box の挿入が変換を入れるためである。translate がタプルに使う配置 (フィールドはどれも `tobj`) と同じ形で、配置の表になければ、handler の段がその回の複製を作る前に、使う大きさの分を足す
- f のブロックを順に写す。H 宛ての `perform` 1つごとに、その操作の節の入口より後ろのブロックと、続きのブロック C を、`perform` のブロックの後ろに足す。どのブロックも番号の大きいブロックへ向かうので、前向きの辺だけの並び (R2) は保たれる。写す前に、元のブロックが新しい並びのどこから始まるかを決め、元の行き先はその位置に向け直す
- 状態は今の値を持つ変数で持ち回る。入口は受けた状態で始まる。元の `jump` の行き先のブロックには状態の引数を足し、どの `jump` も今の状態を渡す。`switch` の行き先は入る辺が1本で引数を持たないので (R3)、`switch` の地点の状態で始まる
- H 宛ての `let r = perform E.op(a…)`: 節の本体を `perform` の地点に展開する。節の変数を新しい変数にすること、捕獲と操作の引数と状態を置き換えること、定数で分ける `switch` を畳むことは、末尾の経路の形と同じである。末尾の `resume k(v, s)` は `jump C(v, s)` にする (状態のない handler では s を省く)。C は r と次の状態を引数で受け、`perform` の後ろの文と終端を続ける。コードは `perform` の数に比例して育つ
- 経路の関数 g の末尾でない呼び出し `let r = call g(args…)`: `g{H}` を、args、操作の節の捕獲、今の状態の順で呼び、g の複製の並びの形で受ける。状態のない handler では結果がそのまま r になる。状態つきで g の元の並びが空なら、結果を次の状態として受け、後ろの r の使いを `()` にする。元の並びが1つなら、受けたタプルを `unpack p (,) #0(r, s')` で開き、s' を次の状態にする。元の並びが2つ以上なら、受けたタプルを開いて s' を次の状態にし、残りの値で r のタプルを作り直す。r は g の結果を開く前の回のコードがすぐ後ろで開くので、作り直したタプルは融合で消える (下の「既知のコンストラクタ」)
- 経路の関数の末尾の呼び出し: 状態のない handler か、g の元の並びが f の元の並びと同じ (Repr と印の並びが等しい) なら、`g{H}` の末尾の呼び出しにする。違えば末尾でない呼び出しとして受け、下の `return` の規則で f の並びの結果にする。タプルの形は値の Repr と印で決まるので、大きさが同じでも並びが違えば結果の形が違うためである
- `return a`: 状態のない handler では `return a` のままにする。状態つきで f の元の並びが空なら、今の状態を返し、a が `unr` でない変数なら先に `drop a` を置く。元の並びが1つなら `con (,) #0(a, 今の状態)` を返す。元の並びが2つ以上なら、a はそのタプルなので、開いてから今の状態を足したタプルを作って返す。タプルの変数の印は結果の印に要素の印を結んだものにする
- handle の地点の `let x = handle …` は、本体の複製 `body{H}` を、本体の捕獲、`()`、操作の節の捕獲、状態つきなら init の順で呼び、結果を上の末尾でない呼び出しと同じく並びの形で開いてから、`return` の節を、その捕獲、本体の結果、最後の状態で呼ぶ。状態のない handler では結果をそのまま本体の結果にして、状態は `()` を渡す。本体の元の並びが空なら本体の結果を `()` にする。足す変数は、本体の `ret` と、`return` の節の状態の引数の Repr と印を写す。文が増えるので、関数ごとに後ろの地点から置き換える

#### 節の写しの、継続についての複製

末尾の経路の形の複製で、それ以外の節の写しに渡す継続は、いつも既知の閉包 `closure f{H}$hN(生きている変数…)` である。そこで、節の写し `<g>{k}` を上の「複製」と同じ規則でその閉包についてさらに複製し、`k` への `apply` を継続の関数の直接の呼び出しにする。閉包を作らず、再開の値と状態を一様な位置に通さないためである。

- 名前は既知の閉包の複製と同じ書き方の `<g>{k}{<位置>=<f{H}$hN>/<捕獲の数>}` で、位置は `k` の引数の位置である。捕獲がなければ `/<捕獲の数>` を省く。64字を超えた名前は、上の「鍵と名前」と同じくハッシュの形にする。継続の関数は `perform` の地点ごとに1つなので、複製も地点ごとに1つで、その地点だけから呼ばれる。複製のコードは g と継続の関数の名前で決まる
- `k` の引数は、継続の関数の捕獲の位置の引数 (生きている変数) に開く。Repr と印は継続の関数の引数から写し、印は `k` の印を超えない
- `k` への `apply` は、上の「既知の閉包」と同じ条件で書き換えられるとき、`call f{H}$hN(捕獲…, v, s)` にする (状態のない handler では s を省く)。`mask` はそのまま残す
- `drop k` は、`unr` でない捕獲の `drop` にする。閉包を捨てると捕獲を手放すのと同じで、`unr` の捕獲は Perceus が手放す
- ほかの使う所 (`Iter k` のように値として使う所や、書き換えられない `apply`) では、その場で閉包を作り直す。`unr` でない `k` は経路ごとに1回しか使われないので、捕獲もそれぞれの経路で1回だけ使われる
- 本体に `handle` を持つ節は複製せず、`<g>{k}` に閉包を渡す。複製すると中の `handle` が複製の数だけ写り、節の関数がちょうど1つの `handle` に属するという EP の前提 (`evidence/handlers.rs` の `entries`) に反するためである

#### handler の特殊化で意味が変わらない理由

- 節の中の `perform` は、もとの意味でも handler の外側で動く。特殊化した後は `evv` に H がないので、外側の handler に届く
- 経路の関数の中の、ほかのエフェクトの `perform` も、外側の handler に届く。H は E だけを扱い、経路に入れ子の `handle` も `mask` もないためである
- 経路の外の関数は、H 宛ての `perform` を起こさない。経路の外へ出るのは、`yields` でない関数の呼び出しと `yields` でない `apply`、状態を渡す形では H に触れない関数の呼び出しだけで、`never` の操作を持つエフェクトでは translate の印も見るためである (上の「handler への特殊化」)。H に触れない関数がほかのエフェクトで yield するときは、上の外側のエフェクトと同じく EP が継続を作る
- 経路の関数の中で外側のエフェクト (`State` など) が yield するときは、EP が継続を作る。その継続には `f{H}` の残りが入るので、H の handle のフレームを含む今の継続と同じ意味になる
- `FileIn` を持つ生産者では、継続の閉包 `closure f{H}$hN` が `FileIn` を捕獲する。継続を捨てると `FileIn` も閉じる。`Iter.generate` の注釈 (継続を捨てるときに閉じる) と同じである
- `never` の操作とそれ以外の節で複製を終えるときに捨てる値は、もとの意味で中断の巻き戻しと `prompt$` が捨てる値と同じである (上の「handler の複製」)
- 状態を渡す形では、状態を変えるのは展開した節だけで、`perform` の順に変える。EP で節が handler の状態を受けて次の状態を返す順と同じである
- 状態を渡す形の経路の関数が外側のエフェクトで yield するときは、今の状態はふつうの生きている変数として EP の継続に捕まる。再開した後も、その変数から続ける
- 中断 (`never` の操作、`try_io` と `try_panic` が受ける失敗、`exit`) は、状態をほかの値と一緒に、巻き戻すフレームから捨てる。EP の巻き戻しが handler の状態を捨てるのと同じである

#### 融合

handler への特殊化と2回目の `yields` の付け直しの後に、融合の段 (`optimize/fuse/`) が動く。規則は、小さな再帰しない関数のインライン化、定数で分ける `switch` の畳み込み、1本の `jump` だけが入るブロックのつなぎ、既知のコンストラクタ、既知の閉包への `apply` の書き換え (上の「既知の閉包」と同じ規則)、既知の閉包をフィールドに持つ `con` を受ける関数の複製、使われない純粋な `let` の削除である。出力は変換の段の verifier を通り、`tail`、`box`、`unbox`、RC の命令を含まない。

- 書き換える順: 段に入った時点の関数の関係を凍らせ、各関数を1回だけ書き換える。直接の呼び出しに値としての参照 (`closure` の関数と関数の値 `&f`) を足した辺で SCC を Tarjan の方法で求め、届く先の SCC から順に書き換える。同じ SCC の中は、直接の呼び出しの関係で呼ばれる関数を先にする。値の参照も辺にするのは、閉包の関数を、その閉包への `apply` を直接の呼び出しにして展開する関数より先に書き換えるためである
- 1つの関数の書き換え: 元の関数は、まず直接の呼び出しを1回の走査で展開する (下の「インライン化」)。そのあと、ブロックの片付け、既知のコンストラクタ、既知の閉包、`con` の値の形についての複製、書き換えで生まれた呼び出しの展開、使われない `let` の削除を、どれも当たらなくなるまで繰り返す
- 使われない純粋な `let` の削除: 展開した本体が返す値を呼ぶ側が捨てる所に、値を作るだけの文が残るためである。純粋の規則は EP と同じ (`tail.rs` の `pure`) で、消しても評価の順と実行時エラーは変わらない。ただし、`Pure` の extern でも、`Lin` の引数を借りずに受ける行 (`Std.MutArray.freeze`) は消さない (`FunctionRow::removable`。[LIR](lir.md) の「縮約」)。既知のコンストラクタを解いて定数になった値の `drop` (`None` の `#0` の `drop`) も消す
- 決定性: どの判断も、その関数のコードと、書き換え終えた呼び出し先と鍵の閉包の関数のコードと、鍵だけで決まる。プログラム全体の大きさの予算は使わない。書き換え終えた関数のコードは名前で決まるので、名前が同じ関数はどのプログラムでも同じコードになる ([VM](vm.md) の「追記」)

#### インライン化

- 展開する本体は、呼び出し先の書き換えた後のコードから写す。その中の呼び出しは、呼び出し先を書き換えたときに展開するかどうかを決めてあるので、呼ぶ側では展開し直さない。展開する本体はどれも `INLINE_LIMIT` 以下なので、最初の走査で関数が育つ分は、その関数の直接の呼び出しの数に比例する。呼び出しの連なりが深くても、深さの積では育たない。元のコードから写して呼ぶ側ごとに展開し直すと、展開の深さの打ち切りがプログラムによって違う所で効き、コードが深さの指数で育つ
- 展開してよい関数は、直接の呼び出しの関係で再帰せず (2つ以上の関数の SCC に入らず、自分を呼ばない)、書き換えた後の文の数が `INLINE_LIMIT` (40) 以下で、`handle`、`resume`、`clause` を持たない関数である。ブロックの終端も1つの文と数える。定数を返す枝の多い `match` も、展開すれば終端の数だけコードが増えるためである。`handle` を持つ関数を展開しないのは、EP が、節の関数がちょうど1つの `handle` に属することを前提にするためである (`evidence/handlers.rs` の `entries`)。しきい値を 40 にした理由は [計測の基準](../implementation/benchmarks.md) の「`INLINE_LIMIT` と `GROWTH_FACTOR`」にある
- 呼び出しの場所でも、展開した本体が Repr と印の検査を通ることを確かめる。変数の実引数は、引数と同じ Repr と印を持たなければならない。印を下げて置き換えると、本体がその引数を `lin` の受け手に渡す所で `unr` と `lin` が出会うためである ([LIR](lir.md) の「Kind の印の検査」の V2)。定数の実引数は、引数の Repr の値になれればよい。本体の `return` の値は、Repr が呼び出しの結果の受け手と互換で、印を受け手に渡せなければならない (`unr` の値は `lin` の受け手に渡せ、逆は渡せない)。yield しうる本体は、yield しうる関数にだけ展開する。`yields` の付け直しは、印の立っていない関数を yield しないものとして扱うためである
- `mask` の付いた呼び出しは展開しない。`mask` は呼び出し先の本体のエフェクトに掛かるので、展開すると掛ける所がなくなるためである
- 最初の走査で展開するのは、書き換え終えた展開してよい関数への直接の呼び出しである。値の参照も辺にした同じ SCC の中の関数でも、直接の呼び出しの呼ばれる関数は先に書き換えてあるので展開する (`Iter.map` のラムダが `map` を呼ぶ形)
- 書き換えで生まれた呼び出し (既知の閉包への `apply` を書き換えた呼び出しと、複製に向けた呼び出し) は、呼ぶ側で新しく展開する。呼び出し先は、コードを読んでよい関数 (下の「融合が止まること」) に限る。同じ SCC の元の関数は、書き換えの順がプログラムによって変わるので読まない。この展開は最初の走査と違って、展開した本体がまた展開する呼び出しを持ち込むので、関数ごとの大きさの予算の中でだけ行う (下の「融合が止まること」)
- 呼び出しのあるブロックは、呼び出しの前の文に本体の入口のブロックを続けたものになり、呼び出しの後ろは結果を引数に受ける新しいブロックに移る。本体の `return r` はそのブロックへの `jump` になる。末尾の位置の呼び出しで、本体の `return` の値が呼ぶ側の `ret` に渡せるなら、`return` のまま残す
- 展開した extern の呼び出しは、もとの位置 (`at`) を持ったまま写す。実行時エラーの位置 (`at path:line:column`) を変えないためである
- Prelude の `|>`、`>>`、`<|` のような合成子も、この規則で展開する

#### 既知のコンストラクタ

同じ関数の中の `con` で作った値への `switch` と `unpack` を、その場で解く。`switch` は選ばれる行き先への `jump` にし、`unpack` はブロックをそこで分けて、残りを `con` の引数を受ける新しいブロックにする。インライン化の後に現れる case-of-case (呼び出しの結果への `match` で、呼び出しを展開したもの) も、この規則とブロックのつなぎで扱う。

- `con` の変数を開くのは、その `switch` か `unpack` が唯一の使いのときだけである。translate が `Unr` でない `con` の変数を開く規則 (上の「変換の規則」) と同じで、引数の所有権はちょうど1回だけ移る。`switch` では、下の jump threading と同じく、選ばれない行き先のブロックの使いも許す。ブロックのつなぎは jump threading より先に動くので、`jump` の側で解くはずだった `switch` が、つないだ後に `con` の変数を直接分ける形になるためである。選ばれない行き先の使いは `switch` を解くと届かなくなって消えるので、所有権が移るのはやはり1回である。`Unr` でない引数は、使われるフィールドに移るときだけ開く。開いた後に残るのは引数の変数だけで、`Unr` でない変数は `decref` できないためである ([LIR](lir.md) の「Kind の印の検査」の V1)
- jump threading: `jump M(a…)` の行き先 M が、文を持たずに引数 `p_i` で分ける `switch` か、`p_i` の `unpack` で始まり、`a_i` が定数か、唯一の使いがこの `jump` である既知の `con` なら、`switch` か `unpack` を `jump` の側で飛ばす。ブロックの分け方は、下の「R3 を守る作り方」の箇条に従う
- `switch` で始まる M では、`p_i` を `switch` のほかの行き先のブロックで使っていてもよい。行き先へ入る辺は `switch` の辺1本だけなので ([LIR](lir.md) の R3)、ほかの行き先の使いは M を通る経路にしか現れない。translate はフィールドを束縛しない葉で `Unr` でない値を `drop` するので (上の「決定木の葉の `drop`」)、`None` の葉が `Option` を捨てていても、`Some` の `con` を `jump` の側で解ける。選ばれる行き先の中で `p_i` を使うときは飛ばさない
- `jump` だけで終わる M (`unpack` と `jump` だけのものを含む) と C は、新しいブロックを作らずに、その `jump` の行き先へ直接つないでよい。ただし、M と C が受けた値 (M の引数とフィールド、C のフィールド) をその `jump` でしか使わないときに限る。M や C の先のブロックがその値を使うと、M と C を飛ばした経路ではその値が定義されないためである。上の規則が `unpack` の位置でブロックを分けると、フィールドを先で使う、`unpack` と `jump` だけの M ができる (状態を渡す handler の複製が返す組を開いて元の組を作り直す所がそうなる)。そのときは「R3 を守る作り方」で分ける。分けた M と C は受けた値を新しい変数で受け直すので、ほかの `jump` は同じ回で直接つなげる
- R3 を守る作り方: `switch` で選ばれる行き先 C は、`jump N(f')` だけを持つ辺のブロックと、元の本体と case のフィールドを引数に持つ合流のブロック N に分け、`jump` を `jump N(fs)` に付け替える。C は `switch` の行き先のまま引数を持たず ([LIR](lir.md) の R3)、N は C の直後に置く。`unpack` で始まる M は、`unpack` と `jump N(…)` だけを残し、残りを M の引数とフィールドを受ける N に移す。パスの終わりにブロックの番号を1回振り直し (R2)、届かなくなったブロックを消す
- 解いた `con` のフィールドにある既知の閉包への `apply` は、上の「既知の閉包」と同じ規則で直接の呼び出しにする。`Iter.next` が `Iter` のフィールドの閉包を呼ぶ所が、この形である

#### `con` の値の形についての特殊化

上の「複製」の「既知」を、既知の閉包をフィールドに持つ `con` の値にも広げる。直接の呼び出しの引数か `closure` の捕獲に、フィールドに既知の閉包を少なくとも1つ持つ既知の `con` があれば、呼び出し先の複製に向ける。

- 鍵は (呼び出し先、引数の位置ごとの形) で、`con` の形はコンストラクタと、フィールドごとの閉包の関数と捕獲の Repr と印か、値のフィールドである。閉包の捕獲と値のフィールドは鍵に入れず、引数にする。値のフィールドは `Unr` の値か定数に限り、レイアウトのフィールドの Repr で渡す。そのため型変数のフィールドのスカラーは、呼び出しの所で `box` され、複製の中で `unbox` される。構造体の `con` の値のフィールドは、配置の Repr でなく形のフィールドの Repr で渡す。複製の中で作り直す構造体の `con` の引数が、形と比べる正確な位置になるためである
- 名前は、呼び出し先の名前の後ろに `{<位置>=<レイアウト> #<タグ>(<フィールド>, …)}` を書く。フィールドは閉包の関数 (捕獲があれば上の「鍵と名前」と同じく `/<捕獲の数>` を付ける) か、値のフィールドの `_` である。捕獲の Repr と印は閉包の関数と捕獲の数で決まり、値のフィールドの Repr と印はコンストラクタで決まるので、名前が鍵を決める。64字を超えた名前は、上の「鍵と名前」と同じくハッシュの形にする
- 特殊化する位置は、呼び出し先が書き換えた後のコードで、その引数を `switch` か `unpack` で開いて閉包として使うフィールド (`apply` する、直接の呼び出しに渡す、`closure` で捕まえる) を持つ位置と、引数を値のまま直接の呼び出しに渡すか `closure` で捕まえる位置である。データに入れるか返すだけの引数を特殊化しても、値を作り直すだけで何も減らないためである。自分を呼ぶ所と包む関数に渡すだけの引数は、渡したとは数えない。自分を呼ぶ所に渡した値は、渡した先でも同じ判断になるためである
- 複製は、呼び出し先の書き換えた後のコードの鍵の位置の引数を、捕獲と値のフィールドの引数に開き、入口で `closure` と `con` を作り直したものである。作り直した値は同じ関数の中の既知の値なので、既知のコンストラクタの規則がその `match` を解き、使われなくなった値は消える。複製は作ったその場で1回だけ書き換える
- 複製のもとにするのは、融合の段に入った時点の関数で書き換え終えたものに限る。複製の複製は作らないので、複製のもとのコードはいつも名前で決まる。包む関数と `handle` を持つ関数は複製しない
- 節の関数は複製してよい。EP は、節の関数がちょうど1つの `handle` に属することを前提にする (`evidence/handlers.rs` の `entries`)。節の関数の閉包を作るのは、その `handle` を持つ関数だけで、その関数は展開も複製もしないので、閉包を作る所は1つのままである。閉包の捕獲の `con` について節の関数を複製すると、その `handle` は節の複製を使い、複製もその `handle` だけに属する
- `Unr` でない `con` は、使う所がすべて開ける位置のときだけ開く。開けない使う所が残ると、捕獲を `con` と複製の引数の2か所で消費するためである (上の「既知の閉包」の `unr` でない閉包と同じ規則)
- 開いた引数の印は、捕獲の位置の印ともとの引数の印の小さい方にする。複製は呼び出し先の `internal` の印を写す (上の「鍵と名前」)。`split` は、複製元の一様な位置のうち、鍵が開いた位置より後ろのものだけを一様に保つ値にする。開いた位置とその前は型どおりにし、元の `split` より前には下げない。開いた位置は葉の並びになり、`split` 以降の引数の数が変わるためである。`split` のない関数の複製は `split` を持たない

`Iter.fold f 0 (Iter.generate (nums n))` では、`generate` の展開で `fold` の引数が既知の `Iter` になり、`fold` がその形について複製される。複製の中で `next` と継続を展開すると `Some (i, Iter (closure …))` がその場で解け、再帰の呼び出しの引数の形が同じ鍵になるので、1要素ごとの確保のないループになる。`Iter.map` を重ねた形も、`map` の閉包の複製を通して同じループに収まる。`Iter.filter` を重ねた形では、残した要素ごとに4つ (`Some`、組、`Iter`、閉包) の確保が残る。`filter` が要素を飛ばすループ (`next (filter p rest)`) は自分を呼ぶ複製になり、再帰する関数は展開しないので、そのループが作って返す `Some ((x, Iter k))` は相手の分からない呼ぶ側に届くためである。これをなくすには、呼び出しの後の継続を関数に切り出し、再帰する呼び出し先をその継続について複製する特殊化が要る。この特殊化はしない ([ロードマップ](../future/roadmap.md) の「処理系」の項目「Core IR の上の最適化パス」)。

#### 融合が止まること

- 元の関数の直接の呼び出しの展開は、呼ばれる関数から先に1回ずつ書き換え、展開した本体を展開し直さないので、深さの上限を置かなくても止まる
- `con` の値の形の鍵は、上の「鍵と名前」と同じく、鍵の入れ子の深さ (`MAX_KEY_DEPTH`) で打ち切る。`con` の形も閉包の形と同じく1段に数える
- 書き換えで生まれた呼び出しの展開は、これだけでは止まらない。展開した本体が、また既知の閉包への `apply` を持ち込むことがあるためである。たとえば、自分を引数に受けて呼ぶ閉包 (`data Fix = | Fix (Fix -> Int -> Int)` を通して自分を呼ぶラムダ) は、展開するたびに同じ形の `apply` が現れる。そこで、この展開は関数ごとに `MAX_INLINE_ROUNDS` 回 (8) までにする。回数はその関数を書き換える間だけ数え、関数のコードだけで決まるので、名前が同じ関数のコードは変わらない
- 回数の上限だけでは、大きさが抑えられない。1回の展開ごとに本体が既知の閉包への `apply` を k 個持ち込むと、関数は回ごとに k 倍に育ち、k の 8 乗の大きさまで育つ。そこで、この展開には関数ごとの大きさの予算を置く。展開した後の関数の文の数 (ブロックの終端も1つと数える) は、書き換えを始めたときの大きさの `GROWTH_FACTOR` 倍 (8) までにする。書き換えを始めたときの大きさが `INLINE_LIMIT` より小さい関数は、`INLINE_LIMIT` の大きさとして数える。呼び出しはブロックの順、ブロックの中の文の順に見て、呼び出し先の大きさが残りの予算に収まるものだけを展開する。予算はその関数自身のコードだけで決まり、プログラム全体の予算は使わないので、名前が同じ関数のコードは変わらない。決まった大きさでなく倍率にしたのは、大きな関数が持つ `Iter` の段の連なりのどれもを融合するためで、下限を置いたのは、数文のラムダにも展開した本体をいくつか入れるためである。8 にした理由は [計測の基準](../implementation/benchmarks.md) の「`INLINE_LIMIT` と `GROWTH_FACTOR`」にある
- 予算だけでも止まらない形がある。展開した本体が解けて同じ形の `apply` に戻ると、関数は育たない。回数の上限は、この形を止める
- 引数の余る既知の閉包への `apply` の書き換えは `apply` を残すが、残る `apply` は呼び出しの結果に向く。それが既知の閉包になるのは呼び出しを展開したときだけなので、上の回数で止まる。ほかの規則は、`switch` や `unpack` を通る経路、`apply`、文のどれかを減らすか、呼び出しを1回だけ複製へ向ける

複製は参照の関係で輪を作りうる。`Iter` の次の状態に自分の閉包を入れる `map` の写しは、値として自分を参照する。互いの閉包を返す2つの関数の写しは、2つの複製の輪になる。書き換えている複製を含む輪は、輪のどの複製を先に作ったかで、まだ書き換えていない所が変わる。そこで、次のように扱う。

- 書き換えている関数から複製への参照を記録し、参照で輪ができたら輪の複製を1つの組にまとめる。参照は書き換えている関数だけが作り、書き換え終えた関数のコードは変わらないので、輪は、輪を閉じる参照を作ったときに必ず見つかる
- コードを読んでよい関数 (展開してよく、鍵の閉包の関数にしてよい関数) は、書き換え終えた関数のうち、元の関数ならいま書き換えている関数と違う SCC に入るもの、複製なら組に書き換えている複製がないものである。書き換えている複製を含まなくなった組は、もう変わらない
- 値として自分だけを参照する複製は、書き換え終えれば展開してよく、鍵の閉包の関数にもしてよい。展開した本体が持ち込むのは自分の閉包だけで、自分を引数に受ける複製は同じ鍵になるためである。`Iter.map` の写しを `Iter.fold` のループに融合するのに要る
- 自分を直接呼ぶ複製と、2つ以上の複製の輪に入る複製は、展開しない。2つ目の条件は決定性のためには要らない。書き換えている複製を含まなくなった組はもう育たないので、そのコードは名前で決まる。安全側の選び方である。輪の複製を展開すると、展開した本体が輪の次の複製の閉包を持ち込み、引く側の複製が輪の複製の数だけできる。標準ライブラリの `Iter` の段はどれも自分だけを参照するので、大きい輪には広げない。そのため、2つの関数が互いの閉包を返す `Iter` を引く `Iter.fold` は融合しない ([実装の現在地](../implementation/status.md) の「最適化が扱わない形」)

#### `yields` の付け直し

書き換えた後のプログラムで、`yields` の印を最小の固定点で付け直す。translate の印が立ち、本体に yield の地点 (再開できる `perform`、`yields` の `apply`、`resume`、`handle`、yield しうる関数の直接の呼び出し) が残る関数だけを yield しうるとする。純粋なラムダで特殊化した `map` の複製は、中に yield しうる呼び出しが残らなければ `yields` でなくなる。

- プログラムの中の `handle` が本体、節、`return` の節として使う関数は、translate の印のままにする。これらの印は、本体の中身でなく handle で決まるためである (上の「`yields` の印」)
- 包む関数の形の関数 (上の「包む関数」) は、プログラムのどこかで値として使われる (`closure f(..)` か `&f`) 間だけ、translate の印のままにする。値はどの文脈からも呼ばれ、ABI は名前で決まるためである。直接呼ばれるだけの関数は、ほかの関数と同じく固定点で印を決める。handle を特殊化した後の `return` の節がこれに当たる。`return x c -> (x, c)` のような `return` の節は包むコンストラクタと同じ形なので、印を保つと、yield しない関数の `yields` の印が呼ぶ側に広がる
- 印を付け直さない関数の集まりは、鍵で決まる。translate は handle の本体、節、`return` の節を、定義ごとにそれぞれ別の関数に持ち上げ、その handle だけが使う。そのため、ある関数がこれらに当たるかどうかは translate の関数では名前で、複製では鍵の呼び出し先で決まり、プログラムのほかの部分に依らない。名前が同じ関数は、どのプログラムでも同じ `yields` の印を持つ ([VM](vm.md) の「追記」)
- 印は立つ方へだけ動く。呼ばれる側が立つたびに呼ぶ側を見直すので、関数と呼び出しの数に比例する手間で済む

#### 直接だけ呼ばれる持ち上げた関数

`optimize` は最後に、届かない関数を落とす前に、値の参照が残らない持ち上げた関数と、handler への特殊化が `k` として渡す継続の関数を、`split` のない関数にする。値として生まれても直接だけ呼ばれる関数が、結果を一様にして呼ぶたびに箱を作らないためである。

- 対象は、translate が `split` を付けた内部の関数 (ラムダ、handle と `var` の範囲の本体、節、`return` の節) のうち、持ち主のあるものである。持ち主は、名前の `$` より前の部分を名前に持つ内部でない関数のうち、名前が最も長いものである (`main$lambda0` の持ち主は `main`)。translate は持ち上げた関数を `<持ち主>$…` と名付け、持ち主と一緒に出す。包む関数 (`$con$`、`$op$`、`$cont$`、`$cont$state`) は、どこからでも参照されるので持ち主を持たせない。そのため、名前を `$` で始め、どの関数の名前に `$` を続けた形にもならないようにする。利用者の関数 `con` があっても、`$con$Prelude.Some@[Int]` の持ち主にはならない。`optimize` の複製と EP が作る関数も対象にしない
- 一族は、持ち主と、同じ持ち主を持つ持ち上げた関数である。一族の `optimize` の後のコードのどこにも値の参照 (`&f` と `closure f(..)`) がない持ち上げた関数を、`split` のない関数にする
- handler への特殊化の末尾の経路の形の複製 `f{H}` も一族を作る。一族は `f{H}` と、それを作るときに作った継続の関数 `f{H}$hN`、合流のブロックの関数 `f{H}$hjJ`、節の写しの複製 `<g>{k}{…}` である (上の「handler の複製」)。一族のコードのどこにも値の参照がない `f{H}$hN` を、`split` のない関数にする。節の写しの複製は `k` への `apply` を継続の関数の直接の呼び出しにするので、`k` を値として使う節 (`Iter k`)、`k` への `apply` を上の「既知の閉包」と同じ条件で書き換えられない節、本体に `handle` を持つ節のどれでもなければ、値の参照は残らない。一族のコードはどれも `f{H}` の名前で決まるので、決め方も名前だけで決まる
- 決め方は名前だけで決まる。一族の関数のコードはどれも名前で決まり、持ち上げた関数があれば一族はそろって translate の出力にあるためである。そのため、名前が同じ持ち上げた関数は、どのプログラムでも同じ ABI を持つ ([LIR](lir.md) の「関数の ABI」)
- 一族の外のコード (持ち主の複製や、持ち上げた関数を鍵にする複製) に値の参照が残っていても、`split` は外す。その参照は、box の挿入が作る版が受ける

#### 検査

debug ビルドでは、`optimize` の後に変換の段の verifier (`verify_translated`) をかけ、誤りを `internal error: invalid Core IR after optimize: …` と報告する (上の「パス」)。translate の線形性の検査 (上の「食い違いがないことの検査」) は translate の中で動き、`optimize` の出力にはかけない。

### evidence passing

エフェクトの操作と handler を、Core IR から Core IR への evidence passing (EP) の変換で実装する。Xie と Leijen の generalized evidence passing (MSR-TR-2021-5。Koka の C バックエンドの方式) を基にする。先行の evidence passing translation (ICFP 2020) は、継続を捕まえたときと同じ handler の文脈でしか再開できないが、この方式はその制限を持たないので、第一級の `k` をそのまま扱える。スタックを切り替えて handler を探す方式 (OCaml 5) は採らない。`multi` の継続にスタックの複写が要り、出力先にスタックを切り替える仕組みが要るためである。変換の後の IR は、普通の呼び出しと分岐、節の呼び出し、決まった表の基本操作だけになる。LIR、VM、ネイティブ化は、`handle`、`perform`、`resume`、`mask` を持たない入力を受け取る。

- EP は translate と box の挿入の間に置く。型は読まない。読むのは、関数の `yields` と `clause` の印、`apply` の印、操作の表、Core IR の形と生存解析だけである
- EP は `tail`、`box`、`unbox`、RC の命令を出さない。Core IR はこれらを持たない (上の「パス」の後のパスの置き場所)
- 書き換えるのは、EP の前からある関数 (translate と `optimize` が作った関数) のうち、`yields` の関数と、`handle`、`perform`、`resume`、`mask` を含む関数である。`yields` でない関数も、row が純粋なまま handle を含むことがある (handle の本体は yield する)。どれにも当たらない関数は変えない
- EP が作る関数 (補助の関数、継続の関数、合流のブロックの関数、入口の関数) は、作るときに最終の形 (yield の確認と遅い道を置いた形) で作り、もう一度は書き換えない
- evidence は実行時にエフェクトの番号で探す ([LIR](lir.md) の「evidence を探す」)。閉じた row の静的な添字と、閉じた row の関数を広い row の文脈から呼ぶときにベクタを合わせる包み (Koka の open) は作らない。トップレベルの関数の row はすべて閉じているので、handler の下での呼び出しの多くが包みを通り、包みは部分ベクタを確保して末尾呼び出しも失うためである。動的に探す費用は handler の数に比例する数命令である
- EP の出力の基本操作、節の呼び出し、入口の関数の印の意味と、それが読み書きする実行の文脈、evidence、ダミーの値、`evv` の不変条件は、[LIR](lir.md) の「基本操作と実行の文脈」にある

#### EP が作る補助の関数

EP は、使うときだけ次の関数をプログラムに1つずつ足す。どれも `yields` の印を立て、`internal` にしない。多くの定義から直接呼ばれるためで、導出の生成器が作る関数を `internal` にしないのと同じ理由である。evidence を受ける引数は `obj`、ほかの引数と `ret` は `tobj` で、どれも一様である。`split` は持たない ([LIR](lir.md) の「関数の ABI」)。

| 関数 | 引数 | 本体 |
|---|---|---|
| `id$` | `x` | `return x` |
| `compose$` | `κ`、`c`、`x` | `r = apply κ(x)`。yield 中なら、`drop r` の後、取り出した κ' と c で `compose$(κ', c)` を κ にしてダミーを返す。そうでなければ `apply c(r)` を末尾で呼ぶ |
| `prompt$` | `ev`、`ret`、`x` | 下の「handle と `prompt$`」 |
| `resume$` | `ev`、`ret`、`κ`、`v`、`s` | 下の「再開と状態」 |
| `rewind$` | `ev`、`ret`、`s`、`κ`、`v` | `resume$(ev, ret, κ, v, s)` を末尾で呼ぶ |
| `mask$[items]` | `κ`、`v` | 下の「`mask` の包み」。`mask` の項目の並びごとに1つ作り、名前に項目を書く (`mask$[State]`、`mask$[Yield@1]`) |

補助の関数の中の `apply` (κ、c、`ret`、節の本体を呼ぶもの) は、どれも `yields` の印を立てる。`apply` はどれも一様で、呼ばれる関数の `split` 以降の一様な位置と合う ([LIR](lir.md) の「関数の ABI」)。

`par` を走らせうるプログラムには、EP は最後に `fork$E.op` と `reperform$E.op` も足す。この2つは上の表の関数と違って `yields` の印を立てず、使うかどうかによらずエフェクトの操作ごとに作る (下の「`par` の子の中断」)。

#### yield の地点

- `yields` の関数の中で、末尾でない次の呼び出しを yield の地点とする
  - `yields` の関数への直接の呼び出し (EP が作る関数を含む)
  - `yields` の印の立った `apply`
  - 再開する操作 (`once` と `multi`) の `Call::Clause`
  - `mask` 付きの呼び出し (下の「`mask` の包み」)。ただし `mask` 付きの `par` は除く
- `par` は、`mask` があってもなくても yield の地点にしない。子が起こせるのは `never` の操作だけで、親はそれを継続を作らずに起こし直すので、子は親へ yield しないためである (上の「Core IR の命令」)
- `never` の操作の `Call::Clause` と `Pure` でない extern (`Panics` と `Effectful`) は、yield の地点にしない。どちらも中断を始めることがあるが、中断は継続を作らずに巻き戻しで進み、呼び出しの後には戻らないためである (下の「`never` の中断」と「IO の extern」)
- 末尾の位置の呼び出し ([LIR](lir.md) の「末尾の位置の呼び出し」の、形だけで決まる定義) は、yield の地点にしない。呼ばれた側の yield は、そのまま呼び出し元へ返るためである。EP はこの定義を Core IR の上の写しで判定する。末尾の位置の `never` の操作の `clause` は、縮約が `tail clause` にする
- yield の地点 `let x = <呼び出し>` の直後でブロックを切り、次の形にする

  ```
    let x = call g(a)
    let y: int = prim yielding()
    switch y { 0 -> b_fast, _ -> b_slow }
  b_slow:
    let d: unit = drop x
    let k0: tobj = prim yield_cont_take()
    let c: tobj = closure f$k1(v1, .., vn)
    let k1: tobj = closure compose$(k0, c)
    let u: unit = prim yield_cont_set(k1)
    return <ダミー>
  b_fast:
    (元のブロックの残り)
  ```

  生きている変数がなければ、`c` は `&f$k1` である。ダミーは f の `ret` のダミーの値である ([LIR](lir.md) の「ダミーの値」)。`ret` が構造体なら、構造体の変数に `prim dummy()` を束縛して返す。その束縛の印は `poly` である。`drop x` は、x が RC の対象 (`unr` でない RC の葉を持つ構造体を含む。[LIR](lir.md) の「構造体の形」) で、印が `unr` でないときにだけ置く。遅い道の x はいつもダミーで、`drop` は何も起こさない。Perceus に `decref` を置かせると、`lin` の x を V1 が拒むためである (上の「EP の明示の `drop`」)
- ブロックを切ると番号が変わるので、EP は関数ごとにブロックを番号の順に作り直し、構造の規則 R1〜R6 を保つ
- 「yield 中か」の確認は、呼び出しごとに文脈の旗を1回読む

#### yield の確認のない呼び出しの後

- 末尾の位置の呼び出しは yield の確認を持たない。LIR の縮約は、結果の Repr が呼び出し元の `ret` と互換なときだけ `tail` にする ([LIR](lir.md) の「末尾の位置の呼び出し」)。そのため、末尾呼び出しの結果は変換を通らずに呼び出し元へ返り、yield 中の値も κ に入らない
- これが正しいのは、次の2つが成り立つためである。この2つを不変条件とする
  - yield の確認のない呼び出しの後には、結果の変換と RC の命令と `return` しか続かない
  - κ に入るフレームの関数 (`f$kN`、`compose$` などの補助の関数) は、再開で値を受ける位置と結果が一様なので、再開で届く値はすでに一様な Repr で、飛ばした変換は要らない

#### 継続の関数

- yield の地点 p ごとに、関数 `f$kN` を1つ作る。N は f の中の yield の地点の通し番号で、ブロックと文の順に振る
  - 引数は、p の後で生きている変数 (x を除き、変数の番号の順) と、x である。`ret` は f の `ret` と同じである
  - 印は `internal` と `yields` で、`split` は最後の1個の引数の手前である。再開の値 x と結果が一様で、ほかの引数は型どおりの Repr である ([LIR](lir.md) の「関数の ABI」)。状態は受けない
- 本体は、p の直後からの区間の写しである。区間は p の直後から辺をたどり、次のどちらかに当たる所で止める
  - 次の yield の地点 q: 呼び出しと確認を同じ形で置く。遅い道は `f$kq` を κ に足して返し、速い道は `let r = call f$kq(..)` と `return r` にする。後で縮約が `tail` にするので、継続の関数どうしの速い道はフレームを積まない
  - 合流のブロック J (`jump` で入るブロック): 区間の中の `jump bJ(args)` を、`let r = call f$jJ(..)` と `return r` にする。`f$jJ` の引数は、J の入口で生きている変数 (J の引数を除き、変数の番号の順) と J の引数で、`ret` は f と同じである。`f$jJ` は、区間から届く J についてだけ作る。直接呼ばれるだけなので `internal` にしない。継続のコードからだけ末尾で呼ばれるので、すべての引数を `split` より前に置き、結果だけを一様にする。`ret` は f と同じ型の一様な Repr である ([LIR](lir.md) の「関数の ABI」)
- 区間の根は yield の地点と合流のブロックである。根でないブロック (`switch` の行き先) は入る辺が1本なので、各ブロックは元の関数と、多くとも1つの区間にしか写されない。そのため、写す文の数は元の約2倍に収まる。これとは別に、遅い道が生きている変数を捕獲する。その大きさは yield の地点ごとの生きている変数の数の和で、yield しうる要素を n 個並べたリストのリテラルのような形では n の2乗になる。根ごとに続きを最後まで写す形では、写す文の数そのものが yield の地点の数と関数の大きさの積になる
- `f$kN` と `f$jJ` の変数の表は、本体に現れる変数 (引数、束縛、使用) だけを持ち、元の関数での番号の順を保って0から番号を詰める。名前、Repr、印は元の関数の表から写す。元の関数の表を丸ごと写すと、表の大きさの和が yield の地点の数と元の変数の数の積になり、上の約2倍の見積もりが表では成り立たないためである
- 元の関数 f は、速い道をそのまま続けるので、yield の地点の確認と遅い道を足すだけである。論文の bind-inlining は f の速い道に、join-point sharing は `f$jJ` に当たる
- 生きている変数は、生存解析 ([LIR](lir.md) の「生存解析」) で求める。捕獲は、ほかのクロージャと同じく `f$kN` の引数の型どおりの Repr のまま持つ
- `f$kN` は値として参照され、直接も呼ばれる。直接の呼び出しは x を一様な Repr で渡し、box の挿入が変換を入れる

#### 継続の組み立て

- κ は、いつも普通のクロージャである。yield を始めるとき、κ は `&id$` である
- 普通のフレームは、上のとおり `compose$(κ, c)` で κ を作り直す。κ を先に呼び、その結果で c を呼ぶ
- 包むフレーム (`prompt$` と `mask` の包み) は、κ を最後の捕獲に持つクロージャ (`rewind$`、`mask$`) で κ を作り直す。再開されると、包むフレームが `evv` を整えてから κ を呼ぶ
- 再開は `apply κ(v)` で、内側のフレームの続きから順に動き、値は外側へ返る
- `once` の継続は `Lin` の値を持ってよく、`multi` の継続は持ち越し規則で `Unr` の値だけを持つ。共有されたクロージャへの `apply` は中身を写すので ([LIR](lir.md) の「関数の ABI」)、`multi` の継続を2回再開しても写すのは捕獲だけである

#### handle と `prompt$`

translate の `let r = handle E(init, body) { c_0, .., c_n } return ret` を、次の形にする。

```
  let w0 = prim evv_get()
  let ev = prim ev_new[E](init, w0)
  (操作ごとに) let u_i = prim ev_set_clause[E.op_i](ev, <i 番目の in-place の入口>)
  let w1 = prim evv_push(w0, ev)
  let u = prim evv_set(w1)
  let x = yields apply body(()) prompt ev
  let r = call prompt$(ev, ret, x)
```

- `w0` と `ev` は2回以上消費するので、Perceus が `dup` を足す。`ev_set_clause` は `ev` を借りるだけなので、数に入らない ([LIR](lir.md) の「基本操作」)
- `apply body(())` は yield の地点にしない。yield したかは `prompt$` が見る。`call prompt$` は、handle を含む関数が `yields` なら yield の地点である (その関数が `yields` でなければ、その row から外へ出る yield はない)
- `apply body(())` には着地の印 `prompt ev` を付ける ([LIR](lir.md) の「着地の印」)。`ev` 宛ての中断は、VM の巻き戻しがこの `apply` の結果 x にダミーを書いて止まり、続く `prompt$` が状態 2 の経路に進む ([VM](vm.md) の「巻き戻し」)。印を付けるのは EP だけで、付ける所はここと、下の「再開と状態」の `resume$` の2か所である
- `prompt$(ev, ret, x)` は、最初に `w = ev_take_evv(ev)` で `ev` の外側の `evv` を取り出し、`evv_set(w)` で `evv` に戻してから、`yield_status(ev)` で4つに分ける。外側の `evv` の参照は `dup` せずに `ev` から `evv` へ移る。`evv` を戻すときに、今の `evv` を手放す。中断が着地したときの `evv` は中断を始めた所の `evv` のままで意味がないが ([LIR](lir.md) の「`evv` の不変条件」)、ここで正しい `evv` に戻る
  - 0 (yield していない): `st = ev_take_state(ev)` で状態を取り出し、`apply ret(x, st)` を末尾で呼ぶ
  - 1 (`ev` 宛ての yield): `drop x` の後、`κ = yield_cont_take()`、`c = yield_finish()`、`st = ev_take_state(ev)` を得て、`k = closure resume$(ev, ret, κ)` を作り、`apply c(k, st)` を末尾で呼ぶ
  - 2 (`ev` 宛ての中断): `drop ret` と `drop x` の後、`c = yield_finish()` と `st = ev_take_state(ev)` を得て、`apply c(st)` を末尾で呼ぶ。中断は κ を組み立てないので、取り出す κ はない
  - 3 (ほかの宛先の yield): `drop x` の後、状態を `ev_take_state(ev)` で取り出し、κ を `rewind$(ev, ret, st, κ)` のクロージャで作り直して、ダミーを返す。ほかの宛先の中断は、巻き戻しがこのフレームを着地せずに外すので、`prompt$` に届かない。EP は状態 3 の経路を中断と yield で分けずに作るが、中断では通らない
- どの道でも `prompt$` は外側の `evv` と状態を取り出すので、`prompt$` を通った evidence (退いた evidence) は入口の表と tag だけを持ち、状態と外側の `evv` の欄は `()` である。退いた evidence を使うのは `resume$` と `rewind$` だけで、`ev_renew` で表と tag だけを読む (下の「再開と状態」)。継続が捕まえた退いた evidence から走っている evidence に届かないことを、`par` の fork が頼る ([オブジェクトモデル](object-model.md) の「evidence の3つの物体」)

#### 再開と状態

- `resume$(ev, ret, κ, v, s)` は、`ev2 = ev_renew(ev, s)` で新しい evidence を作り、今の `evv` の先頭に `ev2` を足して `evv` にする。その後 `x = apply κ(v) prompt ev2` を呼び、`prompt$(ev2, ret, x)` を末尾で呼ぶ。着地の印の evidence は新しい `ev2` で、再開した計算の中の `ev2` 宛ての中断がこの `apply` に着地する。deep handler の再接続と、スコープの外での再開に当たる。`ev2` の外側の `evv` は再開した時点の `evv` なので、handle が返った後の再開も、ほかの handler の中での再開も、その場の文脈の上につながる
- `rewind$` は、外側のフレームの yield で handler の外へ出たときに `prompt$` が取り出した状態で、`resume$` を呼ぶ
- 再開した計算の中の操作は、`ev_lookup` で `ev2` を見つける。古い `ev` を持ったまま止まっているフレームはない。`ev` を持つのは、`prompt$` (止まるときは `rewind$` が `ev` の写す部分だけを使う)、`resume$` のクロージャ (同じ)、yield を始める入口 (止まらない)、in-place の節 (本体に yield の地点がないので止まらない) だけだからである
- handler の実行中にその handler 自身の継続を再開する形も、deep handler の意味 ([エフェクトと handler](effects.md) の「handler の意味」) のとおりに動く。次のプログラムでは、`k r 100` の再開が状態 100 の新しい evidence を作り、外側の実行の evidence (状態 1) は変わらないので、101 を表示する (`tests/ui/run/effects/resume_inside_own_handler.em`)

  ```haskell
  data Re = | Re (Re -> Int -> Int)

  effect G where
    multi grab : Unit -> Re
    get : Unit -> Int
    put : Int -> Unit

  body : Unit -> <G> Int
  body () =
    let r = grab ()
    let n = get ()
    if n == 0 then
      put 1
      match r with
        | Re k -> let inner = k r 100 in inner + get ()
    else n

  run : Unit -> Int
  run () =
    handle body () from 0 with
      | ctl grab () k st -> k (Re k) st
      | ctl get () k st -> k st st
      | ctl put n k _ -> k () n
      | return x _ -> x
  ```

- translate の `Call::Resume { k, v, s }` は、`apply k(v, s)` にする。`k` は `resume$` のクロージャ (直接の形) か、`$cont$` と `$cont$state` のクロージャ (包む形) である。`resume` の `mask` は下の「`mask` の包み」の形にする
- `drop k` は、そのままクロージャの破棄になる。継続のクロージャが捕まえていた値は、解放の連鎖で1回ずつ解放される

#### `perform` と節の3つの形

translate の `let r = perform E.op(a_1, .., a_n)` を、次の形にする。`yields` の関数の中で末尾でなければ、再開する操作の `Call::Clause` は yield の地点になる。`never` の操作の `Call::Clause` はならない (上の「yield の地点」)。

```
  let ev = prim ev_lookup[E]()
  let c = prim ev_clause[E.op](ev)
  let r = clause E.op c(ev, a_1, .., a_n)
```

in-place の入口は、translate が作った節の関数 g から、EP が節ごとに作る。節は次の3つの形に分かれ、EP は Core IR の形から決める。

- **すぐに再開する節**: 次をすべて満たす節である
  - `never` の操作の節でない
  - g の `k` の引数の使いが、末尾の位置の `mask` のない `resume k(v, s)` だけである
  - g のすべての `return` が、そのような `resume` の結果を返す
  - g の本体に、yield の地点になる呼び出しがない。EP は translate の出力の上でこれを決めるので、translate の命令で書くと次のどれもないことである: 再開する操作の `perform`、g が `yields` のときの `handle`、`mask` 付きの呼び出し、最後の末尾の `resume` のほかの `resume`、`yields` の関数への末尾でない直接の呼び出し、印の立った末尾でない `apply`。`mask` 付きの呼び出しは、g が `yields` でなく、その呼び出しが yield の地点にならないとき (下の「`mask` の包み」) も保守的に数える。`never` の操作の `perform` と `IO` の extern の呼び出しは数えない。中断は巻き戻しで進み、節の本体を継続に入れないためである (下の「`never` の中断」と「IO の extern」)

  入口の関数 `g$entry` (`clause` の印) は g の写しである。引数は、捕獲、evidence `ev`、操作の引数の順にし、g の `k` と状態の引数を除く。状態は入口の先頭の `ev_take_state(ev)` で得る。末尾の `let r = resume k(v, s)` と `return r` を、`ev_set_state(ev, s)` と `return v` に替える。`yields` の印は g の印を写す。g は使われなくなるので消す

  g の本体が、最後の末尾の `resume` のほかに呼び出し (`Rhs::Call`。直接の呼び出し、`apply`、`perform`、`handle`) か `Pure` でない extern (`Panics` と `Effectful`) を1つでも持てば、入口を under の形にする。入口の先頭 (状態を取り出す前) で `evv` を evidence の外側の `evv` に替え、各 `return v` の前 (`ev_set_state(ev, s)` の後) で元に戻す。

  ```
    let w = prim evv_get()
    let o = prim ev_evv(ev)
    let u = prim evv_set(o)
    let s = prim ev_take_state(ev)
    (本体)
    let u1 = prim ev_set_state(ev, s1)
    let u2 = prim evv_set(w)
    return v
  ```

  under の形の本体は、yield で handler に移ってから動く節 (下の再開する節。`prompt$` が `evv` を handle の外側の `evv` に戻してから呼ぶ) と同じ `evv` で動く。本体の中の `perform`、`handle`、`mask` は handle の外側の row の handler を見る。本体の中で起きた中断 (`fail`、IO の失敗、`Panic`) も、handle の本体の中の `to_result`、`try_io`、`try_panic` ではなく、handle の外の handler に届く。本体は yield しないので、再開の後に evidence を探し直す仕組みは要らない。中断が入口のフレームを外すときは、入口が持つ `w` を巻き戻しの表が手放す ([VM](vm.md) の「巻き戻し」)。そのときの `evv` は、着地した `prompt$` が正しい値に戻す ([LIR](lir.md) の「`evv` の不変条件」)

  何も呼ばず、`Pure` の extern だけを使う節 (`State` の `get` と `put`) は、under の形にしない。`evv` を読む命令がなく、中断も起こさないので、`perform` の場所の `evv` のまま動かしてよい。数を数える節 (`fn tick () st -> ((), st + 1)`) は `+` が `Panic` を起こしうるので under の形になり、桁あふれは handle の外の `try_panic` に届く
- **再開する節** (`once` と `multi` の操作のうち、上に当たらない節): 入口の関数は、捕獲、`ev`、操作の引数を受け、`c = closure g(捕獲, 操作の引数)` を作り、`yield_to(ev, c, &id$)` を呼んで、ダミーを返す。入口はいつも yield を始めるので、g の印によらず `yields` の印を立てる。`prompt$` が後で `apply c(k, st)` を呼ぶ。g は `k` と状態を受ける最後の2個の引数と結果が一様である。yield しうる外のエフェクトを呼ぶすぐに再開する節もここに入り、節の本体は handle の外側の `evv` で動く。under の形は、本体が yield しないすぐに再開する節だけに使う。yield する本体を under で動かすと、yield と再開のたびに `evv` を替え直す包みが要る。そのうえ再開で evidence を作り直すと、包みと節の本体が古い evidence を持ち続けるので、探し直す仕組みも要るためである
- **`never` の節**: 入口は c を同じ形で作り、`yield_abort(ev, c)` を呼んで中断を始める。最初の κ は渡さず、`yields` の印も立てない。中断は κ を組み立てず、VM がタスクのスタックを巻き戻すので、入口を呼んだ側に yield は返らないためである (下の「`never` の中断」)。`prompt$` が着地の後に `apply c(st)` を呼ぶ。g は状態を受ける最後の1個の引数と結果が一様である。入口の `ret` は `obj` で、`yield_abort` の後に `prim dummy()` を返す形のままにする。`yield_abort` は戻らないので、この `return` には実行が届かない。`never` の操作の `clause` の束縛は、`OpRet::Never` なので位置と比べない。`clause` の後へ制御は戻らないので、その束縛を使う所にも実行は届かない
- handle の場所では、節の値 c_i がどの関数のクロージャかを知る必要がある。translate は節を `&g` か、同じ関数の中の `closure g(..)` で渡すので、EP はそれを読む。そうでない形は内部の誤りとしてパニックする
- `ev_set_clause` には、入口の関数のクロージャ (捕獲がなければ関数の値) を渡す。捕獲は節の捕獲をそのまま写す。再開する節と `never` の節の入口が作る c も、捕獲も操作の引数もなければ `&g` にする (`closure` は引数を1つ以上とる)

#### `never` の中断

- 中断は κ を組み立てない。`yield_abort` が yield の状態に宛先の evidence と節の本体を入れると、VM がタスクのスタックを、着地の印の evidence が宛先と同じ `apply` まで巻き戻す ([VM](vm.md) の「巻き戻し」)。外すフレームが持つ値は、巻き戻しの表で手放す。`FileIn` はこの解放で閉じる
- 途中のフレームは、生きている `Lin` の値に `decref` も `drop` も置かない。生きている値は、巻き戻しの表に載るためである
- 中断を始めうる命令 (`never` の操作の `clause` と `Pure` でない extern) は yield の地点にせず、その後に yield の確かめと遅い道を置かない (上の「yield の地点」)。`IO` と、すべての操作が `never` のエフェクトは、yield しうるラベルでもない ([型と Kind](types.md) の「row の具体化の記録」)。そのため、それらだけを row に持つ関数は `yields` でなく、その中で中断が起きてよい。巻き戻しは `yields` の印によらず、表で途中のフレームを片付ける
- `prompt$` の中断の経路は、`return` の節のクロージャ `ret` を `drop` する。捕まえていた `FileIn` はこの解放で閉じる (上の「EP の明示の `drop`」)

#### IO の extern

- `IO` を起こす extern (表の行の `purity` が `Effectful`) と `Panic` を起こす extern (`Panics`) の呼び出しは、yield の地点でない。直後に yield の確認も遅い道も置かない。`try_io` が捕まえる種類の誤りで失敗すると、VM が `io_failed` の節の入口を呼んで中断を始める。`Panic` を起こす誤りで失敗したときと `panic` は、`panicked` の節の入口を呼んで中断を始める。`exit` はいつも中断を始める ([VM](vm.md) の「extern の中断」)。どの中断も巻き戻しで進み、extern の後には戻らない (上の「`never` の中断」)
- `IO` は yield しうるラベルでないので、`IO` だけを row に持つ関数は `yields` でない ([型と Kind](types.md) の「row の具体化の記録」)。`yields` でない関数も `IO` と `Panic` の extern を呼んでよい
- すぐに再開する節の条件 (「`perform` と節の3つの形」) では、`Pure` でない extern の呼び出しを yield の地点に数えない。そうした extern を呼ぶ節の入口は under の形にするので、節の本体は handle の外側の `evv` で動き、節の IO の失敗と `Panic` は handle の本体の中の `try_io` と `try_panic` でなく、handle の外の `try_io` と `try_panic` に届く
- extern の関数を包む関数 (`$externN`) は、行が `Effectful` でも `yields` でない。extern を呼んで返すだけだからである
- `IO` と `Panic` は、Prelude の最初の2つのエフェクトとして、いつもエフェクトの表にある。番号は 0 と 1 である。`try_io` の handle、`io_failed` の節、`prompt$` は、普通の `never` の操作の handle と同じ形に変換する

#### `mask` の包み

translate の `mask` 付きの呼び出し (直接の呼び出し、`apply`、`resume` から作った `apply`) を、囲む関数が `yields` なら次の形にする。`items` は呼び出しの `mask` の項目の並びで、`prim evv_mask` の即値と `mask$` の名前にそのまま渡す。

```
  let w0 = prim evv_get()
  let w1 = prim evv_mask[items](w0)
  let u = prim evv_set(w1)
  let x = <mask を外した呼び出し>
  let y: int = prim yielding()
  switch y { 0 -> b_fast, _ -> b_slow }
b_slow:
  (κ を mask$[items](κ) で包み、その上で f$kN を compose$ で足して、ダミーを返す)
b_fast:
  let u2 = prim evv_set(w0)
  (元のブロックの残り)
```

- 遅い道は、κ を `compose$(mask$[items](κ), f$kN)` にする。`mask$` が内側のフレームの再開のときに `evv` を整え、`f$kN` が f の残りを続ける
- `f$kN` の区間は、`evv_set(w0)` の後から始まる。`w0` は区間で生きていないので捕獲しない。再開した後の `evv` は、外側の包むフレームが作り直した `evv` で、`mask$` が戻るときにそれへ戻すので、f の残りはその `evv` で動く
- `mask$[items](κ, v)` は、再開されたとき、`w = evv_get()` を得て、`evv` を `evv_mask[items](w)` にし、`r = apply κ(v)` を呼ぶ。yield していなければ `evv` を `w` に戻して r を返す。yield 中なら、`drop r` の後、もう一度 `mask$` で包んでダミーを返す
- 囲む関数が `yields` なら、呼ばれる関数が `yields` でなくても、`mask` 付きの呼び出しを yield の地点にする。遅い道で κ を `mask$` で包むためである
- 囲む関数が `yields` でなければ、項目のエフェクトはどれも yield しない (`Fail` のように操作がすべて `never` のエフェクトの項目など)。包みは `evv_get`、`evv_mask[items]`、`evv_set`、呼び出し、`evv_set(w0)` だけにし、yield の確かめと遅い道を置かない
- `mask` 付きの末尾の位置の呼び出しの後には `evv_set(w0)` が続くので、末尾呼び出しにならない

#### `par` の子の中断

`par` の子が起こせるのは `never` の操作だけである。子の中で起きた操作は子の中で中断し、両方の子が終わった後に親で起こし直す。そのために EP は、エフェクトの操作ごとに `fork$E.op` と `reperform$E.op` の2つの関数を作る。

- 作るのは、プログラムが `par` を走らせうるときだけである。`Call::Par` か `prim root_get` を含むプログラムがこれに当たる (`eml_lir::Program::may_run_par`)。REPL の束縛は前の入力の `par` を走らせる閉包を持てるので、`root_get` も条件に入れる。どちらも含まないプログラムでは、EP の出力は変わらない
- 作る対象のエフェクト (`eml_lir::fork_effects`) は、操作を1つ以上持ち、`EffectInfo::never_only` が真のものすべてである。`never_only` は、どの操作も `never` のエフェクトで真になり、操作のないエフェクトでも真である。extern のエフェクトだけは、操作でなく `eml_extern` の表の行の多重度で決める ([型と Kind](types.md) の「Kind」)。そのため `Prelude.Panic` は入り、操作の `io_failed` が `never` でも多重度が `Once` の `Prelude.IO` は入らない
- 操作のないエフェクトは never-only だが、作る関数がない。VM が子の evidence のために持つ表は、操作のないものを含めて never-only のエフェクトすべてを覆い、操作のないエフェクトの表は空である。子の `evv` が親の never-only の evidence と同じ数の evidence を持ち、子の中の `mask` が飛ばす数がそろうためである
- 名前は、エフェクトの表の名前と操作の名前から `fork$E.op`、`reperform$E.op` とする (`fork$Prelude.Panic.panicked`)。REPL のセッションのエフェクトは版を持つ名前なので、名前に版も入る (`fork$E#2.op`)。中身は操作の表の行だけで決まるので、名前が同じ関数はいつも同じコードになる。VM はこの名前で関数を引く
- どちらの関数も `internal` でなく、`yields` でない。`fork$E.op` は `split` を持たず、プログラムの中から呼ばれず、値としても参照されない。`reperform$E.op` は、`fork$E.op` が作る閉包として値で参照される
- `fork$E.op` は `clause (E, op)` の印を持つ入口の関数である。引数は evidence の `ev` (`obj`、`unr`) と操作の引数 `a_1, .., a_k` (操作の表の Repr と印) で、戻り値は `obj` (`poly`) である。本体は操作の引数を捕まえた閉包 `c = closure reperform$E.op(a_1, .., a_k)` を作り、`prim child_abort(ev, c)` を呼んでから `prim dummy()` を返す。操作の型は関数型なので (`Unit -> ..` の操作も `unit` を1つ受ける)、k は 1 以上である。`child_abort` は戻らないので、この `return` に実行は届かない
  - 閉包の印は、EP が作るほかの閉包と同じく `a_i` の印の結びである (上の「EP が足す変数の印」)
  - `ev` の持ち主は入口である。`clause` の呼び出しはオペランドをすべて消費するので、入口は evidence を所有して受け取る。`child_abort` は `ev` を借りるだけなので、Perceus は `ev` の `decref` を `child_abort` の後に置き、そこへは実行が届かない。`ev` は `child_abort` の巻き戻しの地点が持つ値になり、子のフレームを巻き戻すときに手放される ([LIR](lir.md) の「巻き戻しの地点」)
- `reperform$E.op` は、操作の引数 `a_1, .., a_k` (操作の表の Repr と印) と、最後に `u` (`unit`) を受ける。戻り値は `tobj` (`poly`) である。`split` は `u` の手前 (k) にある。操作の引数は閉包の捕獲なので型どおりの Repr のままで、実行系が閉包に `()` を適用するので、`u` と結果が一様である ([LIR](lir.md) の「関数の ABI」)。値として参照する関数の規則 ([LIR](lir.md) の「verifier」) をそのまま満たすので、box の挿入は版 `<f>$uniform` を作らない
  - 本体は `ev_lookup[E]`、`ev_clause[E.op]`、`clause` で操作を起こす。普通の `perform` と同じく、今の `evv` の最初の `E` の evidence で起こす
  - `Prelude.Panic` は `extern Prelude.panic(a_1)` で起こし直す。位置は持たない。`try_panic` がなければ、`panic` と同じく実行時エラーになる
- 実行系は、親の文脈で `reperform$E.op` を呼ぶ前に、子の操作が届いた handler の evidence が `evv` の最初の `E` の evidence になるように `evv` を整える。`reperform$` は深さを受け取らず、整えた `evv` の最初の `E` の evidence で起こす。fork と join の手順と、子の evidence の作り方は VM の側が決める ([VM](vm.md) の「`par` の fork と join」)

## Core IR から LIR への変換

- `to_lir(eml_core_ir::Program) -> eml_lir::Program` は、関数、ブロック、変数の表、配置の表、文字列の表、エフェクトの表、ファイルの表を、番号と名前を変えずに写す。入口も同じ関数を指す。関数の印 (`internal`、`yields`、`clause`)、`split`、構造体の形の表と `extern` 命令の位置も写す。Core IR の `ret` と `ret_mark` は、LIR の値1つの `rets` になる
- 写すだけで、命令を足しも消しもしない。`handle`、`perform`、`resume` か空でない `mask` が残っていれば、内部の誤りとしてパニックする。EP の段の verifier が先に拒む形なので、捨てて先へ進むことはしない
- 配置の表は、変換で LIR に移る。変換の直後の LIR は、RC の命令、`box`、`unbox`、`tail` を持たない。LIR のパスがそれを入れる ([LIR](lir.md) の「パスの順番」)
- 変数の名前と番号が変わらないので、EP の後の Core IR の表示と、それを写した LIR の表示は同じ文字列になる。2つのテキスト形式は、共通の命令を同じに書く ([テスト戦略](../implementation/testing.md) の「Core IR のテキストの形」)。`crates/eml_core_ir/tests/lowering.rs` が、UI テストのすべてのプログラムでこれを確かめる

## verifier

- Core IR の verifier は2つの段を持つ。段はパスの順に並ぶ
  - 変換の段 (`verify_translated`) は translate と `optimize` の後にかける。[LIR](lir.md) の「構造の規則」の印のない部分と、引き継ぐ検査を確かめる。EP が作るもの (REPL の根の `root_get` と `root_set` と、`var` の節の `empty_mut_array` を除く `prim`、`clause`、`clause` の印の関数) を拒む (`` `prim evv_get` is used before the evidence pass ``、`a clause call is used before the evidence pass`、``a clause of `Counter.get` is defined before the evidence pass``)
  - EP の段 (`verify_evidence`) は EP の後にかける。変換の段の検査から EP が作るものの拒否を除き、基本操作と `clause` の検査 ([LIR](lir.md) の「verifier」) を足す。`handle`、`perform`、`resume` と、呼び出しの `mask` を拒む (``a perform remains after the evidence pass``。`a handler`、`a resume`、`a mask` も同じ形)
- 着地の印 ([LIR](lir.md) の「着地の印」) も EP が作るものなので、変換の段は拒む (`a prompt marker appears before the evidence pass`)。EP の段は、`apply` のほかの検査の後に、印の変数 v が見えること、印の付いた `apply` が末尾の位置 ([LIR](lir.md) の「末尾の位置の呼び出し」) にないこと、v の Repr が `obj` であること、v が呼び出しの直後で生きていることを、この順に確かめる (``an apply with the prompt marker `ev.2` is in a tail position, which leaves no frame to land on``、`` `ev.2` is the prompt marker of an apply, but it is tobj, not obj ``、`` `ev.2` is the prompt marker of an apply, but it is not used after the call ``)。生きているかは、生存解析の入口の集合を関数ごとに1回だけ求め、ブロックの終端と後ろの文から逆にたどって決める。印の変数は呼び出しの使いではないので、生存解析は印を使いに数えない
- 印の検査は、Core IR の2つの段で、`jump` と `return` の V2 と、作る所の検査を行う。それ以外の受け渡しは、境界に合うようになってから LIR の範囲の段で比べる。`perform`、`handle`、`resume` の受け渡しは比べず、EP がそれらを `clause` と基本操作に置き換えた後の段で比べる。`never` の操作の `perform` (変換の段) と `clause` (EP の段) の結果を束縛する変数は、使いに制御が届かないので、V2 と作る所の検査の対象にしない。V1 は `dup` と `decref` が Core IR にないので走らない ([LIR](lir.md) の「Kind の印の検査」)
- `par` は2つの段のどちらでも受ける。`mask` を持てるのは変換の段だけである。組の構築子の検査は LIR と同じ部品で、同じ文言である ([LIR](lir.md) の「verifier」)。子の Repr と印は、境界に合うようになってから LIR の範囲の段で比べる
- RC の命令、`box`、`unbox`、`tail` は Core IR の型にないので、verifier は確かめない。境界の検査もしない。translate と EP の出力は、box の挿入より前には境界に合わないためである
- 構造体の `con`、`unpack`、タグの `switch` の規則 ([LIR](lir.md) の「構造の規則」) は、2つの段で確かめる
- R7 と R8 の文言、`jump` と `return` の文言、引数のない `con` の文言、2つの IR に共通する引き継ぐ検査、検査の順、支配木の求め方は、LIR の verifier と同じである ([LIR](lir.md) の「verifier」)。文言と支配木の求め方などの部品は、2つの verifier が `eml_lir::check` で共有する。検査の順は共有する部品ではなく、2つの verifier で同じに保つ約束である
- Core IR だけの引き継ぐ検査は次のとおりである。どれも変換の段にだけ現れる。EP の段は、これらの命令そのものを拒むためである
  - 呼び出しの `mask` は、エフェクトの番号がエフェクトの表にあり、真に昇順に並び、どの項目も1つ以上の handler を飛ばし、`handle`、`perform`、`clause` に付かない (上の「変換の規則」)
  - `perform` の `resumable` が、エフェクトの表と一致する
  - `handle` の節の数が、エフェクトの操作の数と同じである。`handle` の節と `return` の節の関数が分かるとき (関数の値か、同じ関数の中で `closure` で作った値のとき) は、節が捕獲の後にちょうど「操作の引数 + `k` (再開する操作) + 状態」個の引数を持つことと、`return` の節が捕獲の後にちょうど2つ (値と状態) の引数を持つことも確かめる。操作の引数の数は、エフェクトの表の操作の `params` の長さである
