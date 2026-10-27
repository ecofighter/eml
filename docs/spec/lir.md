# LIR

位置づけ: 規範。

EP の後の Core IR から写す LIR と、その実行の意味を定める。LIR は、box の挿入、開く、縮約、Perceus、reuse が書き換える層で、[VM](vm.md) と後のネイティブ化の入力である。2つの IR に共通する規則 (ブロックの列、構造の規則、値の表現、データの配置、構造体の形、位置の規則、末尾の位置の呼び出し、基本操作と実行時のモデル) はこの文書に1回だけ書き、[Core IR](core-ir.md) から引く。型から決める規則と translate の規則は [Core IR](core-ir.md) にある。物体の形、ヒープ、参照カウントは [オブジェクトモデル](object-model.md) で扱う。

## LIR

### LIR の命令

関数は基本ブロックの列で、ブロックは引数、文の列、終端を持つ。

| 種類 | 命令 |
|---|---|
| 文 | `let x = 右辺`、`let (x1, .., xn) = <呼び出し>`、`unpack v L #t(f1, .., fn)`、`let (y1, .., yn) = unbox w L`、`dup x`、`decref x`、`release x L #t(p1, .., pn)` |
| 右辺 | 呼び出し (`call`、`apply`、`clause`、`par`)、`closure`、`con`、`const`、`extern`、`prim`、`drop`、`box`、`unbox` |
| 終端 | `return a`、`return (a1, .., an)`、`tail <呼び出し>`、`jump bN(a1, .., an)`、`switch a L { #0 -> bN, #1(y1, .., yn) -> bN, _ -> bN }`、`switch a { 1 -> bN, _ -> bN }` |
| RC (RC の対象の変数のみ) | `dup x`、`decref x`、`release x L #t(p1, .., pn)`、`release unboxed x L #t(p1, .., pn)` |
| 手放す | `drop x` (参照を1つ手放す) |
| reuse (token) | `let r: token = release x L #t(p1, .., pn)`、`con L #t(a1, .., an) reuse r`、`drop_token r` (下の「reuse」) |
| evidence passing の出力 | `prim` (基本操作)、`clause` (節の呼び出し) |

- LIR は、EP の後の Core IR を写したものである ([Core IR](core-ir.md) の「Core IR から LIR への変換」)。Core IR との違いは次の2つである
  - エフェクトの命令 (`perform`、`handle`、`resume`) と、呼び出しの `mask` を持たない。LIR の型にないので、EP の後に残っていないことを verifier でなく型で保証する
  - RC の命令 (`dup`、`decref`、`release`)、`box` と `unbox`、末尾呼び出し `tail` を持つ。どれも LIR のパス (box の挿入、縮約、Perceus) だけが入れる。複数の値の `let` と `return`、構造体の `unbox` は開くパスだけが入れる (下の「開く」)。token の命令は reuse のパスだけが入れる (下の「reuse」)。変換の直後の LIR はこれらを持たず、`passes_until` の入口の verifier がそれを確かめる (下の「パスの順番」)
- 関数は、名前、内部の関数かどうかの印 (`internal`)、一様な位置の始まり (`split`)、yield しうるかの印 (`yields`)、入口の関数の印 (`clause`)、変数の表、結果の並び (`rets`)、ブロックの列を持つ。印と `split` は Core IR と同じ意味である ([Core IR](core-ir.md) の「`yields` の印」、下の「関数の ABI」と「入口の関数の印」)。box の挿入は `split` と `clause` を読み、`internal` は読まない。`internal` は ABI を決めない。VM は `yields` を読まない ([VM](vm.md) の「呼び出しと戻り」)
- プログラムは、関数、入口、文字列の表、配置の表、構造体の形の表、エフェクトの表、ファイルの表を持つ。Core IR のプログラムと同じ欄である。エフェクトの表は、`clause` の ABI (操作の Repr) と、基本操作の即値のために持つ
- `Call::Par { left, right, pair, at }` は `par` の fork と join である。テキストの形は `par (,) #0(f.1, g.2)` で、`extern` と同じく位置を後ろに書ける (`par (,) #0(f.1, g.2) @"main.em":3:5`)
  - `left` と `right` は `()` を受ける関数の値で、`apply` の呼ぶ値と同じく `tobj` の位置である。どちらも消費する
  - 結果は、2つの子の値を `pair` の構築子に入れた組で、`obj` である。`pair` は `tobj` のフィールドをちょうど2つ持つ構築子である
  - 末尾の位置の呼び出しにならない (下の「末尾の位置の呼び出し」)。子の中断を親で起こし直すので、巻き戻しの地点になる (下の「巻き戻しの地点」)。子を走らせて両方を待つ動作は [マルチコア対応の設計](../future/multicore.md) の「`par` の意味」に、VM の手順は [VM](vm.md) の「`par` の fork と join」にある
- 2つの IR は別の型を持つ。型を読むだけでどちらの IR か分かり、LIR にエフェクトの命令がないことと、Core IR に RC と box がないことを、verifier でなく型で保証できるためである。重なる部品 (共有する型、テキスト形式の字句解析と共通の命令の読み書き、verifier の支配木と R7 と R8 の検査、基本操作と `clause` の検査) は `eml_lir` に置き、`eml_core_ir` が借りる

### ブロックの列

- 変数の表は、変数の番号ごとに名前、Repr (下の「値の表現」)、Kind の印 (下の「Kind の印」) を持つ
- `blocks[0]` が入口で、その引数が関数の引数である。ほかのブロックは番号 `bN` で指す
- 関数の結果 `rets` は、直接の呼び出しの結果の Repr と Kind の印の組の並びである。開くパスの前はいつも1つで、その Repr を `ret`、印を `ret_mark` と呼ぶ。開いた後は、構造体を返す関数がその葉を並べる (下の「開く」)。テキストの形では、2つ以上の値を `-> (float, float)` と書く
- 文は、変数を定義する `let`、複数の値の `let`、`unpack`、構造体の `unbox` と、RC の命令 `dup`、`decref`、`release` である。ブロックの最後に終端が1つある
- 値 (アトム) は、変数、`Int` の定数、`()`、引数のないコンストラクタのタグ `#N`、`Float` の定数 (ビット列 `FloatBits` で持つ)、関数の値 `&f` のどれかである
- `con` は引数を1つ以上とる。フィールドのないコンストラクタの値は、`#N` (`Atom::Tag`) の1つの書き方にそろえる
- `con L #t(a1, .., an)`、`unpack`、`release` の `L #t` は、配置 L のタグ t のコンストラクタ (`Ctor`) を指す (下の「データの配置」)。タグの case を持つ `switch` は、scrutinee の後に配置 L を1つ持ち、case のタグ `#N` はその配置のタグである
- `jump bN(a1, .., an)` は並列な代入である。実引数をすべて読んでから、行き先の引数に書き、行き先の先頭へ進む
- `switch` は scrutinee、case の並び、省略できる `default` を持つ。case と `default` を合わせて、行き先を1つ以上持つ。case は、タグ (`#N`)、`Int`、`String` (文字列定数の番号) のどれかで比べ、1つの `switch` の case は同じ種類にする。リテラルの `switch` はつねに `default` を持つ。`#N(x, ..)` の case だけがフィールドを束縛し、リテラルの case はフィールドを持たない。同じ case を2回書かない。合う case がなければ `default` に進む
- `unpack v L #t(f1, .., fn)` は、コンストラクタが1つで、そのコンストラクタがフィールドを持つ配置の値を分解する。行き先が「ブロックの残り」である1つの case の `switch` と同じ意味である
- `switch` と `unpack` は、タグとフィールドを読むだけである。参照の数を変えず、scrutinee を消費しない。scrutinee の所有は、どの行き先でも、`unpack` の後でも、`switch` と `unpack` の前と変わらない。フィールドのある case、フィールドのない `#N` の case、`default`、文字列の `switch` のどれも同じである。`int` と `enum` のリテラルの `switch` は、何も所有していない
- `Case` のフィールドと `unpack` のフィールドは、同じ「フィールドの束縛」の規則に従う。Repr が `obj` か `tobj` のフィールドは、束縛したときには借りた値である。自分の参照を持たず、上の持ち主が参照を持っている間だけ使える (下の「verifier」)。ほかの Repr のフィールドは、ただの値である
- `release x L #t(p1, .., pn)` は、分解した値 x を手放す。`unpack` と同じく、フィールドの位置ごとに、参照を引き継ぐ変数か `_` を書く
  - x は、タグが t でフィールドが n 個のデータでなければならない。違えば内部の誤りである
  - x が一意 (参照の数が1で、不死でない) なら、x の箱を解放し、`_` の位置のフィールドの値を `decref` する。名前を書いた変数は、箱が持っていた参照をそのまま引き継ぐ
  - そうでなければ、名前を書いた位置のフィールドを `dup` してから、x を `decref` する
  - 所有の上では、x の参照が1つ減り、名前を書いた変数の参照がそれぞれ1つ増える
  - 名前は1つ以上書く。1つも残さないときは `decref x` を使う
  - Perceus だけが出す RC の命令である。`Stmt::defs` は空で、`dup` や `decref` と同じく値の使いとは数えない
- `let b = box a` は、箱を要する Repr (下の「値の表現」) の値 a を箱に入れる。スカラーの箱は `tobj` の値で、構造体の箱は `obj` の値である。`let v = unbox w` は、箱 w から箱を要する Repr の値を取り出す。種類は、`box` ではオペランドの Repr から、`unbox` では束縛の Repr から決まる。オペランドと束縛の規則は「値の表現」にある
  - `box` はオペランドを消費し、所有した箱を定義する。構造体の葉の参照は箱へ移る
  - `unbox` はオペランドを読むだけで、所有権を受け取らない。借りたフィールドも読める
  - 構造体の `box` と `unbox` は、box の挿入の後の LIR にだけあり、開くパスが箱の `con` と構造体の `unbox` に替える (下の「開く」)
- `let (x1, .., xn) = <呼び出し>` は、複数の値を返す直接の呼び出しの結果を、値の順に束縛する。`return (a1, .., an)` は、複数の値を返す。どちらも開くパスが構造体の結果から作り、開いた後の LIR にだけある。値が1つなら `let x = <呼び出し>` と `return a` で書く
- `let (y1, .., yn) = unbox w L` は、構造体の箱 w (`obj` か `tobj`) のフィールドを読み、葉の変数 y に束縛する。L はコンストラクタが1つの配置である。読まないフィールドは `_` と書く。葉は `unpack` のフィールドと同じ「フィールドの束縛」の規則に従い、Repr が `obj` か `tobj` の葉は w から借りた値である。開くパスが構造体の `unbox` から作り、開いた後の LIR にだけある (下の「開く」)
- `release unboxed x L #t(p1, .., pn)` は、構造体の `unbox` で葉を借りた箱 x を手放す `release` である。x がダミーなら何もしない (下の「ダミーの値」)。ほかは `release` と同じである。Perceus と reuse のパスが、構造体の `unbox` が読む箱に置く `release` にだけ印を付ける。ふつうの `release` の速い道に、ダミーを確かめる分岐を足さないためである
- `drop a` は、a の参照を1つ手放す。a を消費し、結果は `unit` である。意味は `decref` と同じ「参照を1つ手放す」で、「すぐに破棄する」ではない。`poly` の値は実は `Unr` で共有されていることがあり (多重の継続を2回再開して写した `resume$` のクロージャが捕まえる `ret` など)、手放した後に残りの参照が使われうるためである。a が `unr` でない変数でも、`drop` は消費なので、Perceus はその後に何も足さない。`unr` でない値を手放す命令は、`drop` と Perceus の `release` だけである (下の「verifier」の V1)。a がダミーかタグなら、何も起こらない (下の「ダミーの値」)
- 値 (アトム) の使い方は「消費」と「読む」に分かれる。`switch` の scrutinee、`unpack` の値、`unbox` のオペランド (構造体の `unbox` の箱を含む)、`prim` と extern の借用の引数 (下の「基本操作」と「extern の借用」) だけが「読む」で、参照を受け取らない。ほかの使い方 (呼び出し、`apply`、`clause`、呼ばれる側、extern と `prim` の借用でない引数、`con`、`closure` の引数、`drop`、`box`、`return`、`tail`、`jump` の実引数) はすべて「消費」で、参照を1つ受け取る。Core IR のエフェクトの命令の引数も「消費」である
- `tail <呼び出し>` (`TailCall`) は、縮約が作る末尾呼び出しの要求である。呼び出し元のフレームを積まずに呼ぶ。呼び出しの結果は、呼び出し元の `rets` と互換である (下の「末尾の位置の呼び出し」)。LIR の呼び出しは `mask` を持たない。EP が `mask` を `evv_mask` の包みに置き換えている ([Core IR](core-ir.md) の「`mask` の包み」) ためである。所有の都合で `let r = <呼び出し>` と `return r` に戻す降格は、借用パラメータの最適化が、末尾の位置の呼び出しの輪の上にない末尾呼び出しにだけ使う (下の「末尾の位置の呼び出し」、[ロードマップ](../future/roadmap.md) の「段の列」)

### 構造の規則

ブロックの列は次の規則を満たす。verifier が検査する (下の「verifier」)。[LIR だけ] の印を付けた部分は LIR だけに当たり、Core IR は印のない部分に従う ([Core IR](core-ir.md) の「Core IR の命令」)。[境界] の印を付けた部分は境界の検査で、LIR の境界の段だけで走る。

- R1: 入口のブロックへ向かう辺はない
- R2: 辺はすべて、番号の大きいブロックへ向かう
- R3: `switch` の行き先 (case と `default`) は引数を持たず、入る辺はその `switch` の辺1本だけである。合流するブロック (`jump` で入るブロック) は、`switch` の行き先にならない
- R4: 入口と `switch` の行き先を除くブロックには、`jump` が1本以上入る。`jump` の実引数の数は、行き先の引数の数と同じである
- R5: 変数は、関数の中で多くとも1回定義される。パスが消した変数のように、変数の表には定義のない番号が残ってよい。定義する位置は、ブロックの引数、`let`、複数の値の `let`、`unpack` のフィールド、構造体の `unbox` の葉、case のフィールドである
- R6: 変数が見えるのは、その定義の位置が使う位置を支配するときだけである (同じブロックなら前の文)。[LIR だけ] 所有の検査の段では、合流するブロックに入るすべての `jump` で所有の多重集合が一致しなければならず、その集合が入口の所有になる
- R7: Repr の規則 (下の「値の表現」と「位置の規則」)
  - `extern` 命令の引数の数は、`eml_extern` の表の行の `params` の数と同じである
  - `extern` 命令の引数と結果の Repr は、行の Repr が `tobj` でない位置では行と同じである (`tobj` の位置は、境界の段の互換の関係で比べる)
  - `unpack` の値は Repr が `obj` か構造体の変数である。構造体の `unbox` の箱は Repr が `obj` か `tobj` の変数である
  - `jump` の実引数と行き先の引数、`return` の値と `ret` は、互換の関係で比べ、定数は互換の位置の当てはめで比べる
  - [LIR だけ、境界] 直接の呼び出しと `apply`、`clause`、`prim`、`extern` の引数と結果、`closure` の引数は、Repr を互換の関係で比べ、定数は互換の位置の当てはめで比べる。印は送り手から受け手へ渡せるかで比べる (`Mark::passes_to`。下の「Kind の印の検査」)。`apply` の呼ばれる値と引数は `tobj` と比べ、結果は `tobj` である (下の「関数の ABI」)。`tail` の結果は、呼び出し元の `rets` と互換の関係で比べる (下の「末尾の位置の呼び出し」)。`never` の操作の `clause` の結果と、その束縛の使いは比べない。`clause` の印の関数の引数と `ret` は、Repr が操作の表と互換で、印は引数が操作の表から関数へ、`ret` が関数から操作の表へ渡せる
  - [LIR だけ] `box` と `unbox` は「値の表現」の規則に従う
- R8: 配置を指す命令は、その配置と合う (下の「データの配置」)
  - 命令が指す配置は表にあり、タグはその配置のコンストラクタの数より小さい
  - `switch` が配置を持つのは、タグの case を持つときだけである
  - `con` の引数の数と、case、`unpack`、`release` のフィールドの数は、コンストラクタのフィールドの数と同じである (`release` は LIR だけにある)
  - タグの `switch` の scrutinee は変数で、その Repr は配置の Repr と同じである。構造体の変数はタグの `switch` の scrutinee にならない。構造体は `unpack` で分ける。`default` のないタグの `switch` は、すべてのコンストラクタを case に持つ
  - リテラルの `switch` の scrutinee は、`Int` のリテラルなら `int` に、`String` のリテラルなら `obj` に収まる
  - `unpack` と構造体の `unbox` の配置は、コンストラクタが1つである。`unpack` の値が `obj` なら、配置の Repr は `obj` である
  - `con` で束縛する変数の Repr は、配置の Repr と同じであるか、形の配置がその配置である構造体である
  - 構造体を束縛する `con` の引数と構造体の `unpack` のフィールドは、宣言したフィールドの Repr でなく形のフィールドの Repr と比べる正確な位置である。印だけが違う形の値も収まる (下の「構造体の形」)
  - 宣言した Repr が `tobj` でないフィールドでは、`con` の引数と、case と `unpack` のフィールドの Repr が、その Repr と同じである。定数の引数は、正確な位置の当てはめでその Repr に収まる
  - [LIR だけ、境界] 宣言した Repr が `tobj` のフィールドでは、`con` の引数と、case と `unpack` のフィールドの Repr が `tobj` と互換である。定数の引数は、互換の位置の当てはめで `tobj` に収まる
  - R8 は、それぞれの命令を、その命令が指す配置と比べるだけである。値がどの配置で作られたかは追わない。それを保証するのは translate の型である。そのため、配置の違う値を読む IR も R8 を通りうる。タグかフィールドの数が違えば、検査モードの VM が内部の誤りとして止める。形が同じなら気付かない。`tobj` に入ったヒープの物体を `unbox` する IR も R8 を通り、検査モードの VM がヒープの誤り (`WrongKind`) で止める ([VM](vm.md) の「switch」)

R1 と R2 から、ブロックの番号の順は辺の向きに沿った順 (トポロジカル順) になり、ループはない。R3 から、`switch` の行き先の入口の状態は、その `switch` のブロックだけで決まる。

### 値の表現

- 変数は値の表現 (Repr) を持つ。Repr は Core IR で型から決まり ([Core IR](core-ir.md) の「型の Repr」)、LIR にそのまま写る。Kind は Repr とは別に、印として持つ (下の「Kind の印」)
- Repr は `int`、`float`、`enum`、`unit`、`obj`、`tobj` の6つと、構造体の `struct(S)` である。S は形の表の番号である (下の「構造体の形」)。`obj` はつねにヒープの物体を指す。`tobj` は、即値 (タグか関数の値) かヒープの物体を指す。RC の対象は `obj` と `tobj` の変数で、Perceus はこの変数にだけ RC の命令を付ける。reuse のパスが足す token の Repr `token` は、値ではない (下の「reuse」)
- 局所の変数はスカラーと構造体のまま持ち、呼ぶ側か呼ばれる側が具体的な型を知らない位置 (一様な位置) だけを参照にする。一様な位置の Repr は `tobj` である。Lean の IR と同じ方式である。`int`、`float`、`enum`、`unit` の変数は、VM ではタグのないスロットに、ネイティブではレジスタに置ける。値を受ける位置の Repr は下の「位置の規則」が決め、変数の Repr と互換でなければ、box の挿入 (下の「box の挿入」) が変換を入れる
- 一様な位置との間で `box` と `unbox` を要する Repr を、箱を要する Repr と呼ぶ。`int`、`float`、`enum` (箱を要するスカラー) と構造体である。規則と verifier の文言は、この集合の1つの定義 (`Repr::needs_box`) から作る。Repr を足すときに、規則を1つずつ探して直さずに済むためである。スカラーの箱は `tobj` で、構造体の箱は `obj` である (下の「構造体の形」)。命令なしで `tobj` と行き来するスカラーは `unit` だけである。`obj` と `tobj` の間は、命令なしで渡せる
- `Char` の変数の Repr は `int` で、語はつねに Unicode のスカラー値 (0..=0xD7FF か 0xE000..=0x10FFFF) を持つ。この不変条件を守るのは、字句が確かめた文字のリテラルと、確かめて作る変換の extern と、yield の仮の値 (0) だけである。どのスカラー値も 63 ビットに収まるので、一様な位置の箱はいつも即値になる。`Char` を取る extern は、語がスカラー値であることを前提にしてよい。`Char` に専用の Repr を作らないのは、`int` のままなら、箱、`int` の `switch`、定数がどのパスでもそのまま働くためである
- `Byte` の変数の Repr も `int` で、語はつねに 0..=255 を持つ。この不変条件を守るのは、字句が確かめた `Byte` のリテラル、確かめて作る変換 (`Byte.of_int`、`Num Byte`) と切り捨てて作る変換 (`Byte.wrap`、`Bits Byte`) の extern、`Bytes` を読む行 (`u8` を広げる)、yield の仮の値 (0) だけである。結果が `Byte` の extern の行は 0..=255 を返す ([標準ライブラリ](stdlib.md) の「extern の境界」)。`Byte` の比較、`/`、`%`、`&&&`、`|||`、`^^^` を `Int` の計算に載せられるのは、この不変条件があるからである。どの値も 63 ビットに収まるので、一様な位置の箱はいつも即値になる
- `float` は IEEE 754 の倍精度の浮動小数で、生の 64 ビットにビット列を持つ。`float` の case のリテラルはないので、`switch` の scrutinee にならない (リテラルの `switch` は `int` か `obj` の文字列)。`float` の定数は、`0.0` と `-0.0` や NaN のペイロードを区別するために、値でなくビット列 (`FloatBits`) で持って比べる
- 2つの Repr a と b は、次のどれかのとき互換 (`Repr::compatible`) である
  - a と b が同じ
  - a と b がどちらも参照 (`obj` か `tobj`)
  - 片方が `unit` で、もう片方が `tobj`
  - どちらも構造体で、形の骨組み (配置と、葉の Repr) が同じ (`Structs::compatible`)。印は比べない
- 互換の関係は推移的でない (`unit` と `obj` は互換でない)。どの検査も実際の2つの位置を比べるだけなので、推移律は使わない。2つの位置を1つにつなぐ変形は、つないだ後の2つの位置が互換なときだけ行う。縮約の末尾呼び出しの規則がこれに当たる (下の「縮約」)
- 正確な位置では Repr が同じでなければならない。宣言した Repr が `tobj` でないフィールド、タグの `switch` の scrutinee と配置、`unpack` の値、`con` の束縛と配置、`box` と `unbox` のオペランドと束縛である。正確な位置では、`unit` と `tobj` も行き来しない。Repr が表で決まり、translate の出力も表に合わせて作るためである
- 互換の位置では、互換であればよい。`jump` の実引数と行き先の引数、`return` の値と `ret`、直接の呼び出しの引数と結果、`closure` の引数、`apply` の呼ばれる値と引数と結果 (相手は `tobj`。下の「関数の ABI」)、`clause` の引数と結果 (相手は操作の表の Repr)、`prim` の引数と結果 (相手は基本操作の表の Repr)、`extern` の引数と結果 (相手は extern の行の Repr)、宣言した Repr が `tobj` のフィールド、`split` が一様にする引数と `ret` (相手は `tobj`。下の「関数の ABI」)、`tail` の結果と呼び出し元の `rets` (下の「末尾の位置の呼び出し」) である
  - `jump` と `return` を正確にしても、バックエンドが得るものはない。互換な2つの Repr は、機械の形が同じだからである。正確にすると、後のインライン化と contification が作る形を書けない。たとえば `tobj` を返す `id` を `let t: obj = call id(x)` の所でインライン化すると、`id` の `return x` は、`tobj` の x を `obj` の引数へ渡す `jump` になる
- 定数の当てはめも、位置の種類に従う
  - 正確な位置では、`Int` の定数は `int`、`Float` の定数は `float`、`()` は `unit`、タグ `#N` は `enum` か `tobj`、関数の値 `&f` は `tobj` に収まる
  - 互換の位置では、これに加えて `()` が `tobj` にも収まる。`Int` と `Float` の定数は `tobj` に収まらないので、`let b = box 5` を置いて b を渡す
  - `#N` が `tobj` に収まるのに `enum` の変数が収まらないのは、定数ならコンパイルの時点で `tobj` の形に書けるからである。`Int` と `Float` の定数を同じ扱いにしないのは、`box` が確保しうる (ネイティブの 64 ビットの `Int`、`Float`) からである
- `box a` の a は、箱を要する Repr の変数か、`Int` か `Float` の定数である。束縛の Repr は、a が構造体なら `obj`、そうでなければ `tobj` である。`unit` の変数、`()`、`#N`、`&f`、参照の変数は拒む。どれも命令なしで `tobj` に収まるので、1つの値の書き方を1つに保つ
- `unbox a` の a は、`tobj` の変数である。束縛が構造体なら、`obj` の変数でもよい。束縛の Repr は箱を要する Repr である。スカラーを `obj` の変数から取り出すことは拒む。`obj` はつねにヒープの物体を指し、スカラーを入れた値にならない。定数と、`unit` の束縛も拒む
- `int` と `enum` を明示する理由
  - `box` は確保しうる (ネイティブの 64 ビットの `Int`、`Float`)。所有している `tobj` を `unbox` した後には `decref` が要る。どちらも Perceus と所有の検査に見えなければならない
  - `Int` を命令なしで通すと、「小さい `Int` は `tobj` の即値」を LIR の層で決めることになる。`tobj` の `Int` の表し方は、ランタイムが決める ([オブジェクトモデル](object-model.md) の「語」)
  - `enum` を明示するのは、タグを `tobj` の中でどう表すかを、バックエンドに任せるためである
- `unit` を命令なしにするのは、値が `()` の1つだけで、変換がデータも確保も RC も持たないためである
- バックエンドは次を守る
  - `unit` の値の機械の形は、`tobj` の即値 `()` と同じにする。`unit` を返す関数の戻り値も、この形で返す。呼び出しの結果、`jump`、`return`、`tail` が、`unit` と `tobj` の間を命令なしでつなぐためである
  - `obj` の値は、そのまま `tobj` の値でもある
  - `tobj` の位置に置いた定数 `#N` は、値が N の `enum` を `box` した値と同じである。`unbox` が、どちらの値も同じに読むためである
- 後のバイトコード VM とネイティブ化は、開いた後の変数の Repr と `box` と `unbox` から、スロットかレジスタの種類、確保、RC の操作をすべて IR から読める
- `Lin` の値は2回以上消費されない。`Lin` の値はちょうど1回使われる ([線形性](linearity.md)) ので、消費のための `dup` は要らない。`Lin` の物体の参照の数は、いつも1である
  - そのため、スレッドをまたぐときに `Lin` の物体は印を付けずに所有者を移すだけで済む ([マルチコア対応の設計](../future/multicore.md))。`FileIn` を閉じる時点は `close` や `drop` を書いた所と一致し、extern (`read_all`、`close`) は一意性を前提にできる
  - フィールド (下の項目) のほかに `Lin` の値の `dup` が出る原因はないと見ている。ソースで2回使えば、型検査が `Unr` の制約を付ける。`multi` の継続は、持ち越し規則で `Unr` の値しか捕まえない。EP の継続のクロージャ、ブロック引数、handler の状態は move で、呼ばれる側の閉包は `apply` で消費される
- `Lin` のフィールドも `dup` されない。型の正しいプログラムでは、`Lin` のフィールドの持ち主 (下の「Perceus」) も `Lin` である。データの Kind はフィールドの Kind の上限で決まるためである ([型と Kind](types.md) の「Kind」)。`Lin` の持ち主は分解した後に使われないので、Perceus がフィールドの所有を取るどの時点でも、持ち主はその命令で必要でなく、解かれる。フィールドは `release` で受け取られる
  - 後の行のパターンが分解した値かその祖先を束縛する決定木 (`| other -> ..`、`| W h _ -> ..`) でも同じである。決定木の途中の行き先ではフィールドは借りたままで、所有を取る葉では持ち主が死んでいる
  - この議論は、決定木の途中にガードがないことに依る
- `unr` でない変数の `dup` と `decref` は、V1 が静的に拒む (下の「verifier」)。`FileIn` を含む `Lin` のデータとクロージャの `dup` は、`lin` ビットが立っていないとほかの物体と区別できないので、この静的な検査が覆う。`lin` ビットの立った物体 (`FileIn` など) の参照を増やす違反は、検査モードの `eml_rt` も実行時に見つける ([オブジェクトモデル](object-model.md) の「`lin` ビット」)
- 文字列のリテラル (`const`) の値は、凍結した領域の不死の物体を指す ([オブジェクトモデル](object-model.md) の「凍結した領域」)。それでも LIR は、リテラルについても `dup` と `decref` の釣り合いを保つ。値がリテラルだからといって `dup` や `decref` を消すパスは作らない

### Kind の印

- 変数と関数の `ret` は、Repr と並べて Kind の印 (`Mark`) を持つ。印は値が `Lin` を含みうるかを表す3値である。単相化の後でも、一様な位置の型変数 (`Flexible`)、instance の鍵に入らない矢印の線形性の Kind 変数、EP が多相な関数のフレームの続きとして作るクロージャは、Kind が決まらないためである

  | 印 | 意味 | `dup` | `decref` |
  |---|---|---|---|
  | `unr` | 静的に `Unr` と示せる | 許す | 許す |
  | `lin` | 静的に `Lin` と分かる | 拒む | 拒む |
  | `poly` | 静的には分からない | 拒む | 拒む |

  - `poly` に `dup` を拒んでも困らない。ソースで2回使うか使わなければ型検査が `Unr` の制約を付けるので、制約のない Kind 変数の値はちょうど1回使われている。`decref` を拒むのは、`Lin` の物体の RC が 0 になると破棄処理が呼ばれ、暗黙の後始末になるためである。手放すには `drop` を使う (上の「ブロックの列」)
- 印は Repr と同じく Core IR で決まり ([Core IR](core-ir.md) の「Kind の印」)、LIR にそのまま写る。LIR のパスは既存の変数の印を変えない。box の挿入と開くパスが足す変数の印は、下の「box の挿入」と「開く」にある。縮約は変数を足さない。LIR のパスが足す変数は、Perceus の構造体の箱の `drop` の結果の `unit` の変数 (`unr`) と、reuse の token の変数 (`r`。下の「reuse」) だけである
- 結び (`Mark::join`) は、`lin` > `poly` > `unr` の順で大きいほうである。受け渡し (`Mark::passes_to(送り手, 受け手)`) は、送り手が `lin` で受け手が `unr` の向きだけが偽である。`lin` ビットを落とす向きだけを拒み、`unr` の値を `lin` の受け手に渡すのは、ビットが立たないだけで安全な側である。`poly` はどちらの向きも通す。順序 (`Mark::at_least`) は `unr` < `poly` < `lin` で、作る所の検査が使う。交わり (`Mark::meet`) はこの順序で小さいほうで、構造体の葉の印を決めるのに使う (下の「構造体の形」)
- 印の受け渡しは推移的でない (`lin` から `poly`、`poly` から `unr` はそれぞれ渡せるが、`lin` から `unr` は渡せない)。2つの位置を1つにつなぐ変形は、つないだ後の送り手と受け手の組が渡せるときだけ行う。縮約の末尾呼び出しの規則と、translate の `jump` の転送がこれに当たる (下の「縮約」、[Core IR](core-ir.md) の「変換の規則」)
- 定数 (`#N`、`()`、`Int`、`&f`) はどの印の位置にも渡せる。`None` を `lin` の引数に渡すことはよくある
- 値の流れの比べでは、送り手と受け手を取り違えない。変数を別の変数に替える置き換え (展開の引数、`jump` の転送、複製の引数) は、渡すのでなく替えるので、印が等しいこと (または `at_least`) を求める
- 印を使う検査は、V1、V2、作る所の検査の3つである (下の「Kind の印の検査」)。`unpack` と case で得たフィールドの印は、配置の表がフィールドの Kind を持たないので verifier は確かめず、translate を信用する。ヘッダの `lin` ビットが実行時に覆う ([オブジェクトモデル](object-model.md) の「`lin` ビット」)
- テキストの形は、Repr の後に `unr` でないときだけ印を書く (`x.1: tobj lin`、`-> tobj poly`)。読み手は、書かなければ `unr` にし、`unr` と書いたら誤りにする。書き方を1つにするためである ([テスト戦略](../implementation/testing.md) の「LIR のテキストの形」)

### データの配置

- `Program.layouts` は配置の表で、`LayoutId` で引く。`con`、`unpack`、`release` は配置とタグの組 (`Ctor`) を持つ。タグは配置の中のコンストラクタの添字で、宣言の順である。タグの case を持つ `switch` は配置を1つ持つ。`if` と条件が出す `Bool` の `switch` も含む。リテラルの `switch` と、`default` だけの `switch` は持たない
- タグの定数 `#N` (`Atom::Tag`) と case のタグ (`CasePattern::Tag`) は配置を持たない。配置は `switch` に1つだけ置く
- 配置は、名前と、タグの順に並んだコンストラクタを持つ。コンストラクタは、名前と、宣言したフィールドの Repr の列を持つ。Repr の決め方と、どの型が配置を持つかは、[Core IR](core-ir.md) の「translate の配置」にある
- 名前は、テキストの形と、後の値の表示とデバッグ情報のためにある。名前では何も解決しない
- 配置の Repr は表に持たない。data の Repr と同じく、コンストラクタのフィールドの数から決まる (`data_repr`)。フィールドのないコンストラクタだけなら `enum`、どれもフィールドを持てば `obj`、混ざれば `tobj` である
- 配置の表は、Core IR から LIR に写る。LIR のパスは配置を足さない。配置はフィールドの Repr を持ち、Kind の印は持たない (上の「Kind の印」)
- 宣言した Repr が `tobj` のフィールドは、互換の位置である (上の「値の表現」)。`obj` の値は変換なしで `tobj` のフィールドに置け、`tobj` のフィールドは `obj` の変数に束縛できる。`box` と `unbox` が要るのは、スカラーと参照の間だけである。ただし `unit` は、命令なしで `tobj` のフィールドと行き来する
- verifier の R8 は、命令をその命令が指す配置と比べるだけで、値を作った配置を追わない。それを保証するのは translate の型である (上の「構造の規則」の R8)
- VM はジャンプ表の大きさとポインタのフィールドを、後のネイティブ化は物体の形を、reuse は同じ配置かどうかを、この表から知る ([VM](vm.md) の「switch」、[オブジェクトモデル](object-model.md) の「物体の形」)
- 構造体にする型 (下の「構造体の形」) も配置を持つ。構造体の箱は、その配置の `con` と同じ形の物体である。配置が構造体の型を宣言したフィールドの Repr は `tobj` で、構造体はそこへ箱に入れて置く ([Core IR](core-ir.md) の「translate の配置」)

### 構造体の形

- 構造体の Repr `struct(S)` は、小さい単一コンストラクタの型とタプルの値を、ヒープの箱でなく、葉の値の並びとして持つ表現である。どの型を構造体にするかは translate が型から決める ([Core IR](core-ir.md) の「構造体にする型」)
- `Program.structs` は形の表で、`StructId` で引く。形 (`StructShape`) は、配置と、フィールドごとの Repr と Kind の印の組の列を持つ。フィールドの Repr は具体化した型から決まる。入れ子の構造体は、フィールドの Repr が `struct` になる。テキストの形では番号でなく中身を書く (`Vec3{float, float, float}`、`(,){int, obj lin}`)
- 同じ中身の形は同じ番号にする (`Structs::intern`)。形は具体化した型から決まり、型は instance の名前に入っているので、名前が同じ関数はいつも同じ ABI になる。番号は1つの Program の中だけで意味を持つ。配置の番号と同じである。VM は形を中身で引き直す ([VM](vm.md) の「追記」)
- 形の葉は、フィールドを深さ優先に順に並べたものである (`Structs::leaves`)。`unit` の葉も位置を保つために並びに入れ、語の数には数えない (`Structs::words`)。入れ子の構造体の葉の印は、葉の印とそれを含むフィールドの印の小さいほう (`Mark::meet`) である
- 互換 (`Structs::compatible`) は、形の骨組み (配置と、葉の Repr) が同じときだけ真である。正確な位置の比べ (`Structs::same`) も骨組みで比べる。印は Repr の比べに入れず、葉ごとに印の規則 (上の「Kind の印」) で比べる。状態を渡す形の複製が値の印から作る形と、型から作る形とで、印だけが違うことがあるためである ([Core IR](core-ir.md) の「handler の複製」)
- 構造体は `tobj` とも `obj` とも命令なしで行き来しない。一様な位置へは、構造体の型の配置の箱に入れて渡す。箱はいつもヒープの物体なので、箱の Repr は `obj` である。箱のフィールドは配置の宣言した Repr を持つ。`(Int, Float)` の箱のフィールドはどちらも `tobj` なので葉を `box` してから入れ、`Vec3` の箱のフィールドは宣言どおり `float` なので葉をそのまま入れる
- 構造体は extern の行と操作の表に現れない。どちらも構造体の型の位置を `obj` にする
- 構造体の変数の Kind の印は、型の Kind から決まる ([Core IR](core-ir.md) の「translate が決める印」)。`unr` でない RC の対象の葉を持つ構造体は、後始末の上では RC の対象と同じに扱う (`Structs::rc_target`)。EP の遅い道の `drop x`、決定木の葉の `drop`、EP の明示の `drop` がこれに当たる。開くパスが、構造体の `drop` を葉ごとの `drop` にする (下の「開く」)
- 構造体の Repr は、box の挿入の後まで1つの変数のまま扱う。開くパスが構造体の変数を葉の変数の並びに替え、開いた後の LIR には構造体の Repr が残らない (下の「開く」)

### 位置の規則

位置ごとに、そこへ渡す値に期待する Repr は次のとおりである。正確に合わせるか互換でよいかは、上の「値の表現」の互換の関係で決まる。局所の変数とブロックの引数の Repr と、translate が決める関数のシグネチャは、[Core IR](core-ir.md) の「型の Repr」にある。

| 位置 | 期待する Repr |
|---|---|
| データのフィールド (`con` の引数、case と `unpack` の束縛) | 配置の表の、宣言したフィールドの Repr。型変数と組と構造体の型のフィールドは `tobj`。構造体の `con` と `unpack` は形のフィールドの Repr |
| 直接の呼び出し `call g(..)` の引数と結果 | `g` の引数の Repr と `ret` |
| `extern` の引数と結果 | 表の行の `params` と `ret`。型変数の位置は `tobj` (下の「extern の行」) |
| `apply` の呼ばれる値と引数と結果 | `tobj` (下の「関数の ABI」) |
| `clause` の引数と結果 | 先頭の evidence は `obj`、残りの引数と結果は操作の表の `params` と `ret` (下の「エフェクトの位置」)。`never` の操作の `clause` の結果は値を持たない (下の「`never` の操作」) |
| `prim` の引数と結果 | 基本操作の表の Repr (下の「基本操作」) |
| `closure g(..)` の引数 (捕獲と部分適用) | `g` の引数の Repr |
| `split` が一様にする関数の引数と `ret` | `tobj` と互換 (下の「関数の ABI」) |
| `return a` | 関数の `ret` |
| `tail` の結果 | 関数の `rets` (下の「末尾の位置の呼び出し」) |
| `jump bN(..)` | 行き先の引数の Repr |

- Core IR のエフェクトの命令 (`perform`、`resume`、`handle`) は、box の挿入より前に EP が消すので、この表に位置を持たない
- `split` が一様にする位置を一様にし、位置に合わない値を変換するのは、box の挿入 (下の「box の挿入」) である

#### 関数の ABI

- 関数の引数と `ret` の Repr は、具体化した型から決まる ([Core IR](core-ir.md) の「型の Repr」)。直接の呼び出しは、この型どおりの ABI で呼ぶ
- 関数の値は、捕獲より後ろの引数と結果がいつも一様である。`apply` は呼ぶ値と引数を `tobj` で渡し、結果を `tobj` で受ける。そのため `apply` は、どの関数を呼ぶかを知らなくても、引数を一様な Repr にして呼べばよい
  - 関数の値の ABI を捕獲の数だけで決めるのは、作った所と `apply` する所が違う型を見ることがあるためである。単相化していても、一様な位置 (`_`、多相再帰) の境目では、データの中に入った関数の値も行き来するので、静的には包み切れない。一様な位置どうしなら、どちらの型を見ていても Repr が合う
  - 捕まえた値は、呼ばれる関数の引数の Repr で持つ。`closure g(..)` の引数は直接の呼び出しの引数と同じく、`g` の引数の Repr への変換を入れる位置である
- 一様な位置の始まり (`split`) を持つ関数は、`split` 番目以降の引数と結果が一様で、それより前の位置 (捕獲) は型どおりの Repr のままである。`split` を持たない関数は、すべての位置が型どおりである
  - 値として生まれる関数は、捕獲の数を `split` に持つ。ラムダ、handle と `var` の範囲の本体、節、`return` の節、包む関数 (`$op$`、`$con$`、`$externN`、`$cont$`、`$cont$state`)、継続の関数 `f$kN`、`par` の子の中断を親で起こし直す `reperform$E.op` である。translate と EP が作るときに付ける。`reperform$E.op` の捕獲は操作の引数で、`split` はその数である ([Core IR](core-ir.md) の「`par` の子の中断」)
  - 値として生まれた関数でも、`optimize` の後に値の参照 (`&f` と `closure f(..)`) が1つも残らないものは、`split` を持たない ([Core IR](core-ir.md) の「直接だけ呼ばれる持ち上げた関数」)
  - `prompt$` は再開する操作の節に `k` と状態を、`never` の操作の節に状態だけを渡す (`apply c(k, st)`、`apply c(st)`)。継続の関数 `f$kN` は、生きている変数を捕獲として受け、その後に再開の値を受ける ([Core IR](core-ir.md) の「継続の関数」)
  - EP の合流の関数 `f$jJ` は、すべての引数を `split` より前に置き、結果だけを一様にする。継続のコードからだけ末尾で呼ばれるので、そこで末尾呼び出しを保つためである
  - EP が作る補助の関数 (`compose$`、`prompt$`、`resume$`、`rewind$`、`mask$[…]`、`id$`) は `split` を持たず、引数と結果がすべて `tobj` である ([Core IR](core-ir.md) の「EP が作る補助の関数」)。`par` の子の中断のために EP が作る `fork$E.op` と `reperform$E.op` は、この補助の関数に入らない。`fork$E.op` は `split` を持たない入口の関数で (下の「入口の関数の印」)、`reperform$E.op` は上の値として生まれる関数と同じく `split` を持つ
  - `optimize` の複製は、複製元の一様な位置のうち、鍵が開いた位置より後ろのものだけを一様に保つ。捕獲より前には下げない ([Core IR](core-ir.md) の「`con` の値の形についての特殊化」)。handler への特殊化が作る関数は、継続として渡す `f{H}$hN` のほかは `split` を持たない ([Core IR](core-ir.md) の「handler の複製」)。継続として渡す `f{H}$hN` も、一族のコードに値の参照が残らなければ `split` を持たない ([Core IR](core-ir.md) の「直接だけ呼ばれる持ち上げた関数」)
- 一様な Repr は、箱を要する Repr を `tobj` にし、`obj` と `unit` をそのままにしたものである。構造体も `tobj` の位置で受け、構造体の箱 (`obj`) は命令なしでそこへ渡る
- 結果が一様な関数とは、`rets` が `obj`、`tobj`、`unit` のどれか1つの関数である。値として生まれる関数、版 (下)、`f$jJ`、EP の補助の関数と、結果の型の Repr がそれらになる関数がこれに当たる。ほかの関数を、結果が型どおりの関数と呼ぶ
- 値として参照する関数の、捕獲より後ろの引数と結果が一様でないときは、box の挿入が版 `<f>$uniform` を作り、値の参照をそちらへ向ける (下の「box の挿入」)。トップレベルの関数、ローカル関数、`split` を持たない複製と handler への特殊化がこれに当たる。版は引数と結果がすべて一様で、`split` を持たない。形は2つある
  - 薄い形は、引数を `unbox` して `let r = call f(..)` で `f` を呼び、結果を `box` して返す
  - 写しの形は、`f` のブロックと変数の表を写し、引数と結果を一様にしたものである。`f` から、結果が型どおりの関数への末尾の位置の直接の呼び出しをたどって届く範囲に、末尾の位置の `apply` か `clause` か、結果が `ret` と互換でない直接の呼び出しがあれば、写しの形にする (`needs_copy`)。薄い形の `call f` の下でそれらがフレームを残し、末尾の位置の呼び出しの輪でスタックが伸びるのを防ぐためである。答えは呼ばれる側の本体だけで決まる
  - 結果が一様な関数 (薄い形の版を除く) の中では、結果が型どおりの関数 `g` への末尾の位置の直接の呼び出しを、`g` の版への呼び出しに替える。版の中の呼び出しも同じである。そのため、結果が一様なコードは、末尾の位置で結果が型どおりのコードへ戻らない (下の「末尾の位置の呼び出し」)
  - 版は、`yields`、`internal`、印、文の位置 (`Loc`) を `f` から写し、`clause` の印を付けない。実行時エラーの位置は `f` の位置のままで、版の名前は逆アセンブルにだけ出る
  - 薄い形の `call f` には yield の確認がない。yield 中は `f` がダミーを返し、版はそれを箱に入れて返す (下の「ダミーの値」)。版のフレームは κ に入らず、再開した値は κ を通って `apply` に一様な値として届く ([Core IR](core-ir.md) の「yield の確認のない呼び出しの後」)
  - 元の関数は、値としてしか参照されなくても残す。名前で追記する REPL が、名前で関数を引くためである
- 入口の関数の印 (`clause`) を持つ関数は、`split` でなく操作の ABI に合わせる (下の「box の挿入」)。捕獲は一様な Repr で持つ。`ev_set_clause` の引数として値で参照されるが、`apply` では呼ばれないので、版を作らない
- 内部の関数かどうかの印 (`internal`) は ABI を決めない。`optimize` が包む関数を形で見分けるときと、複製の印を写すときにだけ読む ([Core IR](core-ir.md) の「包む関数」と「鍵と名前」)。テキストの形では、内部の関数を `internal fn main$lambda3(..)` と書く ([テスト戦略](../implementation/testing.md) の「LIR のテキストの形」)
- テキストの形では、`split` を `fn` の直前に `uniform from N` と書く。`split` のない関数には書かない (`internal uniform from 0 fn main$handle1(..)`)
- ABI は具体化した型と `split` だけで決まり、どちらも名前で決まる。そのため、名前が同じ関数はいつも同じ ABI になる。`split` を外す規則は、ラムダを参照しうるコード (親の関数と、名前を引く `optimize` の複製) だけを見るので、そのコードも名前で決まる。版の形も `f` と、`f` から末尾でたどる関数のコードだけで決まる。`optimize` はほかの定義に本体を展開し、既知の閉包への `apply` を直接の呼び出しにするので、ある関数を直接呼ぶ定義と値として使う定義は、プログラムごとに変わる。プログラム全体の使い方で ABI を決めると、名前が同じ関数の ABI がプログラムによって変わる。名前で追記する REPL では、古い入力の関数と食い違う ([VM](vm.md) の「追記」)。ネイティブ化では、翻訳した関数を使い回せなくなる
- クロージャの呼び出しの規約は変えない。共有されたクロージャへの `apply` は、中身を写す (Lean の `pap` と同じ)

#### エフェクトの位置

- エフェクトの表の操作は、引数の Repr の列 `params` と、結果 `ret: OpRet` を持つ。操作のスキームの型から決め、型変数は `tobj` にする。データの配置のフィールドの Repr と同じ考え方である。`effect Counter where put : Int -> Unit` の `put` は `int` を受けて `unit` を返し、`effect State s` の `get` は `tobj` を返す
- `OpRet` は `Repr` か `Never` である。`never` の操作の結果は `Never` で、呼び出した側の位置と比べない (下の「`never` の操作」)。操作の引数の数は `params` の長さで、別には持たない。操作が再開するかどうか (`resumable`) も持つ
- 操作の宣言した Repr は、`clause` の引数と結果、つまり in-place の入口 ([Core IR](core-ir.md) の「`perform` と節の3つの形」) の引数と結果に使う。`clause` の先頭の引数の evidence は `obj` である
- 継続を再開する値と、yield で運ぶ節の引数は一様のままにする。EP の後は継続の再開も yield も `apply` で動き、`apply` は一様なので (上の「関数の ABI」)、そこに操作の Repr を通すには、継続ごとと節ごとに別の呼び出しの規約が要るためである。効くのは再開する操作だけで、すぐに再開する操作 (状態の `get` と `put`) は in-place の入口の境界だけで効く
- translate の出力の `handle` の節と本体、`return` の節は、`split` 以降の一様な位置を持つ (上の「関数の ABI」)。translate の出力の `perform`、`resume`、`handle` のオペランドと結果は、EP が消すので境界の検査を受けない

#### `never` の操作

- `never` の操作の `clause` は値を返さない。`never` の節の入口は中断を始め、中断はタスクのスタックを巻き戻すので、`clause` の後へ制御が戻らない (下の「実行の意味」)。そのため、その結果は位置でなく、束縛の Repr は translate が決めたまま (具体化した型の Repr) にする
  - box の挿入は、`never` の `clause` の束縛を受け直さず、その束縛を使う所にも変換を入れない。束縛の使いには制御が届かないからである
  - 縮約は、末尾の位置の `never` の操作の `clause` を、束縛の Repr によらず `tail` にする。戻らないので、結果を受ける所がない
  - verifier は、`never` の `clause` の結果を束縛と比べず、末尾の `never` の操作の `clause` の結果を呼び出し元の `rets` と比べない。互換の位置では、`never` の `clause` の束縛の使い (`return` の値、呼び出しの引数など) も位置と比べない。正確な位置の使いは、ほかの変数と同じく比べる
  - 縮約の前の `let t = clause Fail.fail ..` と `return t` の形は、t の Repr が `ret` と互換でなくても、そのまま `tail clause` になる。この呼び出しは末尾の位置にあるので、EP は yield の地点にしない
- 結果を `tobj` のような1つの Repr にしないのは、具体化した型の Repr のまま束縛すれば、使う所に変換が要らないためである。`never` の操作の `clause` は戻らないので、末尾呼び出しの保証にも関わらない
- `never` の操作の引数は、ほかの操作と同じく操作の表の `params` に従う。in-place の入口の `ret` は `obj` である ([Core IR](core-ir.md) の「`perform` と節の3つの形」)

#### 戻らない extern

- 表の行の `ret` が `Never` の extern (`exit`、`panic`、`fail_io`) は値を返さない。結果の束縛は位置でなく、Repr と Kind の印は translate が具体化した結果の型から決める。`never` の操作の `clause` の束縛と同じ扱いで、box の挿入は受け直さず、使う所にも変換を入れない。verifier は結果を行と比べず (R7 の結果の項と、Kind の印の比較)、互換の位置での束縛の使いも比べない
- 行の `ret` と `OpRet` は、操作の表と同じ型 (`eml_extern::OpRet`) である

#### extern の行

- extern の行の引数と結果は、直接の呼び出しの引数と結果と同じく互換の位置である。std の宣言の型変数の位置 (`Std.Array.get` の要素、`Std.Array.make` の初期値) は `tobj` である。translate は具体化した型の Repr (`int` など) のまま引数を渡して結果を束縛し、box の挿入が行の Repr との間に `box` と `unbox` を入れる (下の「box の挿入」)
- 境界の段は、引数と結果を互換の位置として行と比べる。Core IR と LIR の入口の段は、引数の数に加えて、行の Repr が `tobj` でない位置の引数と結果の Repr が行と同じかを比べる。`tobj` の位置は具体化した型の Repr のままなので比べない (R7)

#### extern の借用

- 行は、引数ごとの借用の印 (`FunctionRow::borrowed`) を `params` と並べて持つ。印はどの位置の引数にも立てられる。印の立つ引数を extern は読むだけで、消費しない。基本操作の借用の引数 (下の「基本操作」) と同じく読む使いなので、Perceus は借りた変数を `dup` しない。借用が最後の使いなら、その命令の直後で手放す
- 借りるのは、配列を読む4つの行 (`Std.Array.length`、`Std.Array.get`、`Std.MutArray.length`、`Std.MutArray.read`) の配列の引数と、`String` を読む11行の `String` の引数である。11行は、`Std.String.byte_length`、`byte_at`、`char_at`、`slice`、`find_from` (探す文字列と対象の両方)、`Prelude.Eq String.==` と `!=`、`Prelude.Ord String.compare` (両辺)、`Prelude.Show String.show`、`Std.Float.can_parse` と `parse_raw` である。`FileIn` を読む5行 (`Std.FileIn.read_all`、`read_line_raw`、`read_all_bytes`、`read_line_bytes_raw`、`read_chunk`) と、`Stdin` を読む同じ名前の5行 (`Std.Stdin.read_all` など) と、`Child` を読む同じ名前の5行 (`Std.Proc.read_all` など) も、ハンドルの引数を借りる。`read_chunk` は、足す先のバイト列 (最初の引数) を消費する。`Bytes` を読む行 (`Std.Bytes.length`、`get`、`slice`、`find_from` の探すバイト列と対象、`find_byte_from` の対象、`append_slice` の `src`、`Prelude.Eq Bytes.==` と `!=`、`Prelude.Ord Bytes.compare` の両辺、`Show Bytes` が読む `Prelude.bytes_length` と `bytes_get`) も `Bytes` の引数を借りる。`Std.FileOut`、`Std.Stdout`、`Std.Stderr`、`Std.Proc` の書き込みの行 (`write`、`write_bytes`、`write_line`) は書くデータ (最初の引数) を借り、ハンドルを消費して同じハンドルを返す。読むたびに参照を渡して返す手間と、1バイトずつ読むループの `dup` を省くためである。`++`、`Std.String.push`、`println`、`eprintln`、`Std.FileIn.open`、`Std.FileOut.create` と `append`、`Std.Env` の行、`Std.Proc.spawn_raw` と `exited` は、引数を消費したままにする
- 1つの命令が同じ変数を借りて、かつ消費するなら、その命令の前に参照が2つ要る。extern の行でこの形を持つのは `Std.Bytes.append_slice` だけで、`src` を借り、`dst` を消費する。借用は命令の間じゅう続くので、消費に渡す参照とは別に1つ持つ必要があるためである。Perceus は、借りる変数を同じ命令が消費するなら、その変数を命令の後も生きているとみなして `dup` を置き、命令の後の `decref` が借用の分を手放す。verifier は、`dup` のないこの形を `` `e.0` is lent to `prim ev_renew` and consumed by the same instruction `` のように断る。借りるだけの使いと、別の変数を消費する使いは、この規則に当たらない
- extern が失敗して中断しても、借りた引数は呼んだ側が持ったままである。巻き戻しの表は呼び出しの後に持っている値を手放すので、借りた引数もそこで手放す ([VM](vm.md) の「巻き戻し」)。消費する引数は、extern が手放す
- `with_arg: Some(k)` の行 (`Std.FileIn`、`Std.Stdin`、`Std.Proc` のそれぞれ5つの読み、`Std.MutArray.length`、`read`) は、宣言の型では結果と引数 k の組を返す。行の結果は組の最初の要素で、組は translate が作る ([Core IR](core-ir.md) の「組を返す extern」)。LIR の `extern` 命令は行の結果だけを束縛し、借りた引数 k はその後の `con` が消費する
- 縮約は、`Lin` の引数を借りずに受ける `Pure` の行 (`Std.MutArray.freeze`) を消さない (下の「縮約」)

### 末尾の位置の呼び出し

関数の末尾の呼び出しは末尾呼び出し (`tail`) にし、呼び出し元のフレームを積まない。末尾呼び出しは、縮約だけが IR の形から作る (下の「縮約」)。translate がこの形を残す規則は、[Core IR](core-ir.md) の「変換の規則」にある。

- 末尾の位置の呼び出しは、IR の形で決まる。1つのブロックの終端が `return x` で、同じブロックに x を定義する `let x = <呼び出し>` があり、その後の文がすべて純粋な `let` (縮約が消すもの) であるとき、その呼び出しを末尾の位置の呼び出しという。開いた後は、`let (x1, .., xn) = <呼び出し>` の後の終端が同じ並びを同じ順に返す `return (x1, .., xn)` の形も含み、構造体の `unbox` も純粋な `let` に数える。後ろの純粋な `let` の変数は `return` が使わないので、縮約がすべて消し、そこで `tail` を作る。形だけで決まるので、`never` の操作の `clause` も含む。`par` は含まない。2つの子の値を待つ join に、親のフレームが要るためである。EP ([Core IR](core-ir.md) の「yield の地点」)、box の挿入、縮約は、この定義を使う
- EP は Core IR の上で、box の挿入と縮約は LIR の上で、この定義を判定する。判定は同じ定義の写しである。純粋な右辺の集合は、LIR では `box` と `unbox` を含み、Core IR では含まない。EP の出力には `box` と `unbox` がないので、EP の出力の上で判定は同じ答えを返す。EP が「末尾の位置の呼び出しは yield の地点にしない」と決めたことと、縮約が末尾呼び出しを作ることは、この同じ定義の上で成り立つ
- 末尾の位置の呼び出しは、結果の Repr が呼び出し元の `rets` と互換で、結果の印を戻り値の印に渡せるときだけ `tail` になる (下の「縮約」)。互換でなければ、box の挿入がほかの呼び出しと同じく結果を受け直し、呼び出し元のフレームが残る。実行時に結果を変換する末尾呼び出しはない
- 保証: 末尾の位置の呼び出しの鎖ごとに、残るフレームは多くとも1つである。そのため、末尾の位置の呼び出しの輪をたどってもスタックは伸びない
  - 結果が型どおりの関数どうしの末尾の位置の呼び出しは、結果の型が同じなので Repr が合い、`tail` になる
  - フレームが残るのは、結果が型どおりの関数が、結果が一様な関数を末尾の位置で呼ぶ所である。そこから末尾でたどる先は、どれも結果が一様なコードである。結果が一様なコードは、結果が型どおりのコードを末尾で呼ばない。直接の呼び出しは版へ替え (上の「関数の ABI」)、`apply` の結果は `tobj` だからである
- 保証の例外は次のとおりである。どれも深さがほかの数で抑えられる
  - `mask` の包みと `prompt$` の呼び出し。戻りで `evv` を戻すのでフレームを残すが、そのフレームの数は handler の数で抑えられる (下の「`evv` の不変条件」)
  - 操作の表の結果が、節の関数の結果と違う節の入口。同じ操作を節の中で起こすと外側の handler に行くので、深さは handler の入れ子で抑えられる
  - 結果が一様なコードの中の、操作の表の結果が箱を要する Repr の `clause`。`tail` にならずにフレームが残るが、in-place の入口が戻るまでの一時的なものである
- 保証は IR の形についての文である。呼び出しの値が、文を持つ合流のブロックを通って `return` に届く形 (`let r = if n == 0 then 0 else f (n - 1) in let s = "unused" in r`) は、末尾の位置の呼び出しでなく、保証の外にある ([実装の現在地](../implementation/status.md) の「実行の費用」)
- 後のパスもこの保証を保つ。所有の都合の降格は、輪の上の `tail` に当てない (上の「ブロックの列」の `tail`)

## 基本操作と実行の文脈

EP ([Core IR](core-ir.md) の「evidence passing」) の出力は、普通の呼び出しと分岐、節の呼び出し、決まった表の基本操作だけになる。この節は、その基本操作と節の呼び出しの意味と、それが読み書きする実行時のモデルを定める。`eml_rt` の `Ctx` と VM が実装する。

### 実行の文脈

実行の文脈は、次の2つである。VM とネイティブでは、これを実行の単位ごとに持つ。1つの worker で `par` と `Async` のタスクを切り替えるなら ([マルチコア対応の設計](../future/multicore.md) の「1つの worker での実行」)、タスクごとに持つか、切り替えで退避する。`eml_rt` では、実行文脈 `Ctx` が2つを持ち、タスクは yield の状態がなしの安全点でだけ切り替えて、`evv` だけを退避する ([オブジェクトモデル](object-model.md) の「実行文脈 `Ctx`」)。

- `evv`: 今有効な evidence の並び。内側の handler を先頭にした永続リストで、書き換えない。handler を設けるたびに、先頭に足した新しいリストを作る。空のリストは `()` である
- yield の状態: なし、yield 中、中断中のどれか。yield 中と中断中は、宛先の evidence と節の本体のクロージャを持ち、yield 中は組み立て中の継続 κ も持つ。中断は κ を組み立てずにタスクのスタックを巻き戻す (下の「実行の意味」)

`eml_rt` の `Ctx` は、yield の状態を平らな記録 `{ target, clause, cont, abort }` で持ち、宛先 `target` が `()` のときを「なし」とする。ネイティブのコードが、決まった位置の語と即値の1回の比較で、yield 中かを読めるようにするためである。

`evv` を関数の引数で渡す形は採らない。関数の値の呼び出しの規約が「yield しうるか」で分かれ、関数の値として使う関数に evidence の引数を足す一様化が要るためである。`mask` の包みが残すフレームは handler の数で抑えられる (下の「`evv` の不変条件」) ので、文脈に持つ形にした。

### evidence

evidence は、handler の1回の実行を表すヒープの物体で、次を持つ。

- 入口の表: エフェクトの番号と、操作ごとの in-place の入口のクロージャ ([Core IR](core-ir.md) の「`perform` と節の3つの形」) を持つ物体。handle で入れた後は変えない
- 状態の欄 (`tobj`)
- 外側の `evv`: その実行の外側で有効な `evv`。作るときに決め、`prompt$` が取り出すまで変えない。その evidence 自身を含まない

handle の実行と、継続の再開のたびに、新しい evidence を作る。入口の表は handle のときに1つ作り、再開で作る evidence はその表を共有し、状態と外側の `evv` だけを新しくするので、作る費用は操作の数によらない。yield の宛先は evidence の物体そのもので比べるので、目印の番号は持たない。

書き換えるのは状態の欄と外側の `evv` の欄だけである。状態の欄を書き換えるのは、その evidence の実行が動いている間の、in-place の節 (`ev_take_state`、`ev_set_state`) と `prompt$` (状態の取り出し) と、その実行を中断で外す巻き戻し (状態の取り出し) だけである。外側の `evv` の欄を書き換えるのは、`prompt$` の取り出し (`ev_take_evv`) だけである ([Core IR](core-ir.md) の「handle と `prompt$`」)。別の実行は別の evidence を持つので、欄を共有しない。handle 1回につき evidence を1つにして状態を yield と再開で出し入れする形は採らない。handler の実行中にその handler 自身の継続を再開すると ([Core IR](core-ir.md) の「再開と状態」の例)、2つの実行が状態の欄と外側の `evv` の欄を共有して壊し、外側の `evv` がその evidence 自身を指す参照の輪もできるためである。

状態の欄を通る参照の輪はできない。状態は値で、値から届く evidence は退いたものだけであり、退いた evidence の状態と外側の欄は `()` である ([オブジェクトモデル](object-model.md) の「evidence の3つの物体」)。そのため、evidence を通る輪はない。`prompt$` が、どの出口 ([Core IR](core-ir.md) の「handle と `prompt$`」の 0〜3) でも状態の欄を空にすることは、evidence を退かせることの一部である。外側の handler O の本体の中で内側の handler I を動かし、I の継続 `k_I` をデータに包んで O の状態に入れても、`k_I` が捕まえる I の evidence は退いていて、O の evidence に届かない。これを `debug_heap` 付きの UI テスト (`tests/ui/run/effects/continuation_in_outer_state.em`) で確かめる。O の外の宛先への中断と `exit` は `prompt$` を通らない。O の handle のフレームを着地せずに外すときに、巻き戻しが O の evidence の状態を取り出して手放すのは、状態を早く解放するためだけである ([VM](vm.md) の「巻き戻しの手順」)。この場合も UI テスト (`tests/ui/run/unwind/state_holding_inner_continuation_exit.em`、`state_holding_inner_continuation_fail.em`) で確かめる。

### evidence を探す

- `ev_lookup[E]` は、`evv` を先頭からたどり、エフェクトの番号が E の最初の evidence を返す。費用は今有効な handler の数に比例し、呼び出しの深さにはよらない
- `evv_mask[items](w)` は、`items` の各項目 (エフェクト、残す数 n、飛ばす数 m) について、w の先頭から合う evidence を数え、n 個を残して続く m 個を除いたリストを作る。項目のないエフェクトの evidence は残す。除く位置までの節点を写し、残りは共有する。evidence の物体は写さない
- `evv` は書き換えないので、探す場所ごとにキャッシュを置いても結果は変わらない ([ロードマップ](../future/roadmap.md) の「処理系」)

### 基本操作

右辺 `Rhs::Prim(Prim, Vec<Atom>)` は基本操作である。`Prim` は基本操作の種類と、その即値 (エフェクトの番号、操作、`mask` の項目の並び) を持つ。テキストの形は `prim ev_lookup[State]()`、`prim ev_clause[State.get](ev.2)`、`prim evv_mask[State*2, Yield@1](w.3)` のように、即値を `[…]` に書く。`evv_mask` の項目は Core IR の呼び出しの `mask` と同じ型 (`eml_lir::MaskItem`) で、書き方も同じである ([テスト戦略](../implementation/testing.md) の「Core IR のテキストの形」)。

- 引数と結果の Repr は、次の表で決まる。引数と結果は、直接の呼び出しの引数と結果と同じく互換の位置で、合わなければ box の挿入が変換を入れる。状態とクロージャの引数には、translate が決めた Repr の値 (`State Int` の状態なら `int`、状態のない handler なら `()`) が来るためである
- 引数は、表の借用の列が「借用」のものを読み、ほかを消費する。結果は所有した参照である
  - 借用にするのは、基本操作がその引数そのものを物体にも実行の文脈にも入れない引数である。読むだけの引数と、中身の一部を基本操作が自分で `dup` して共有する引数 (`evv_mask` のリストの残り、`ev_renew` の入口の表) がこれに当たる
  - 消費する引数は、extern の行と同じく所有権を受け取る。文脈か物体に入れる引数がこれに当たる
  - 借用の引数は、`unbox` のオペランドと同じく読む使いである (上の「ブロックの列」)。借りた変数をそのまま渡せる。`ev_take_state` と `ev_set_state` は借りた evidence の状態の欄を、`ev_take_evv` は外側の `evv` の欄を書き換える (上の「evidence」)
  - 表は引数ごとの借用の印 (`PrimRow::borrowed`) を `params` と並べて持ち、長さは `params` と同じである
- 表は `eml_lir` に置き (`Prim::row`)、`eml_extern` には入れない。`eml_extern` の表は `std/` と1対1に結び付けるテストを持ち、基本操作はソースに現れないためである。Core IR も同じ `Prim` を使う
- 基本操作は純粋でないとみなし、縮約は消さない。例外は `root_get` と `empty_mut_array` で、根か定数を読むだけなので、結果を使わなければ消す
- `eml_rt` の `Ctx` は、`dummy`、`empty_mut_array`、`child_abort` を除く基本操作をメソッドとして実装し、動作を Verus で証明する ([オブジェクトモデル](object-model.md) の「実行文脈 `Ctx`」)。即値はメソッドの引数になる。`ev_new` は、エフェクトの番号のほかに入口の数も引数で受ける。VM とネイティブのコードは、エフェクトの操作の数をコードの定数として持つためである

| 基本操作 | 引数 | 借用 | 結果 | 動作 |
|---|---|---|---|---|
| `evv_get` | なし | | `tobj` | 今の `evv` |
| `evv_set` | `tobj` | なし | `unit` | `evv` を替える。元の `evv` は手放す |
| `evv_push` | `tobj`、`obj` | なし | `tobj` | リストの先頭に evidence を足す |
| `evv_mask[items]` | `tobj` | 元の `evv` | `tobj` | 上の「evidence を探す」 |
| `ev_new[E]` | `tobj` (状態)、`tobj` (外側の `evv`) | なし | `obj` | 新しい入口の表 (欄は空) と、それを指す新しい evidence |
| `ev_set_clause[E.op]` | `obj`、`tobj` | evidence | `unit` | 入口の表の欄に入れる。handle の場所だけが、本体を始める前に使う |
| `ev_renew` | `obj`、`tobj` (状態) | 元の evidence | `obj` | 元の evidence と同じ入口の表を指し、状態を引数に、外側の `evv` を今の `evv` にした新しい evidence |
| `ev_lookup[E]` | なし | | `obj` | 上の「evidence を探す」 |
| `ev_clause[E.op]` | `obj` | evidence | `tobj` | 入口の欄を読む |
| `ev_evv` | `obj` | evidence | `tobj` | 外側の `evv` を読む |
| `ev_take_evv` | `obj` | evidence | `tobj` | 外側の `evv` を取り出し、欄を `()` にする。`prompt$` の入口で使う ([オブジェクトモデル](object-model.md) の「evidence の3つの物体」) |
| `ev_take_state` | `obj` | evidence | `tobj` | 状態を取り出し、欄を `()` にする |
| `ev_set_state` | `obj`、`tobj` | evidence | `unit` | 状態の欄を替える |
| `yielding` | なし | | `int` | yield 中か中断中なら 1、そうでなければ 0 |
| `yield_status` | `obj` | evidence | `int` | 0: yield していない、1: この evidence 宛ての yield、2: この evidence 宛ての中断、3: ほかの宛先の yield か中断 |
| `yield_to` | `obj`、`tobj` (節の本体)、`tobj` (最初の κ) | なし | `unit` | yield を始める |
| `yield_abort` | `obj`、`tobj` (節の本体) | なし | `unit` | 中断を始め、タスクのスタックを巻き戻す。戻らない (下の「実行の意味」) |
| `yield_cont_take` | なし | | `tobj` | κ を取り出す (欄は空になる) |
| `yield_cont_set` | `tobj` | なし | `unit` | κ を入れる |
| `yield_finish` | なし | | `tobj` | 節の本体を取り出し、宛先の evidence への参照を手放し、yield の状態をなしに戻す |
| `root_get[i]` | なし | | `tobj` | 根 i を `dup` して返す ([オブジェクトモデル](object-model.md) の「セッションの根」)。REPL の translate が出す |
| `root_set[i]` | `tobj` | なし | `unit` | 値を根 i に置く。根 i は空でなければならない |
| `dummy` | なし | | `obj` | ダミーの物体 (下の「ダミーの値」) |
| `empty_mut_array` | なし | | `obj` | 長さ 0 の `MutArray` の不死の物体。`var` の配列の変数を取り出した後の状態で、translate が `take` の節に出す ([Core IR](core-ir.md) の「`var` の変換」)。VM は凍結した領域の定数の語を返す ([VM](vm.md) の「`CodeBase` と `FnEntry`」) |
| `child_abort` | `obj`、`tobj` (起こし直しの閉包) | evidence | `unit` | `par` の子を中断し、操作の引数を捕まえた起こし直しの閉包を親に渡す。戻らない |

- 表は、引数と結果の Kind の印も持つ (`PrimRow::param_marks`、`ret_mark`)。evidence と `evv` の位置は `unr`、状態、κ、節の本体の位置は `poly` である。`ev_set_clause` の入口のクロージャは、`ev_clause` が `dup` して返すので `unr` である。`empty_mut_array` の結果は `MutArray` の値なので `lin` である。`child_abort` の起こし直しの閉包は操作の引数を捕まえるので `poly` である。V2 が `prim` の引数と結果をこの列と比べる (下の「verifier」)

  | 基本操作 | 引数の印 | 結果の印 |
  |---|---|---|
  | `evv_get` | なし | `unr` |
  | `evv_set` / `evv_push` / `evv_mask` | `unr` / `unr`、`unr` / `unr` | `unr` |
  | `ev_new[E]` | `poly` (状態)、`unr` | `unr` |
  | `ev_set_clause` | `unr`、`unr` | `unr` |
  | `ev_renew` | `unr`、`poly` | `unr` |
  | `ev_lookup` / `ev_clause` / `ev_evv` / `ev_take_evv` | なし / `unr` / `unr` / `unr` | `unr` |
  | `ev_take_state` / `ev_set_state` | `unr` / `unr`、`poly` | `poly` / `unr` |
  | `yielding` / `yield_status` | なし / `unr` | `unr` |
  | `yield_to` / `yield_abort` | `unr`、`poly`、`poly` / `unr`、`poly` | `unr` |
  | `yield_cont_take` / `yield_cont_set` / `yield_finish` | なし / `poly` / なし | `poly` / `unr` / `poly` |
  | `root_get` / `root_set` | なし / `unr` | `unr` |
  | `dummy` | なし | `poly` |
  | `empty_mut_array` | なし | `lin` |
  | `child_abort` | `unr`、`poly` | `unr` |

- `ev_set_clause` と `ev_clause` の即値は、操作をエフェクトで修飾して書く (`ev_clause[State.get]`)。テキストの形で操作の名前をエフェクトで修飾するためである
- `yielding` と `yield_status` の結果を `int` にするのは、リテラルの `switch` で分けるためである。`Bool` で分けると、EP が配置の表に `Prelude.Bool` を足すことになり、translate の後のパスは配置の表を変えないという規則 ([Core IR](core-ir.md) の「translate の配置」) を崩す

### 節の呼び出し

- `Call::Clause { effect, op, callee, args }` は、操作の ABI を知っている呼び出しである。テキストの形は `clause State.get c.3(ev.2, ())` である
- `callee` は in-place の入口のクロージャ、`args` の先頭は evidence (`obj`)、残りは操作の引数である。引数と結果の Repr は、操作の表の `params` と `ret` から取る (上の「エフェクトの位置」)
- `apply` と同じく関数の値を呼ぶ。VM とネイティブでは、操作の ABI を知っている間接呼び出しになる ([VM](vm.md) の「呼び出しと戻り」)
- `mask` は持たない
- 操作の Repr を `perform` の境界に通すために、この呼び出しを置く。box の挿入、縮約、verifier にその規則がある。Perceus には規則を足さない

### 着地の印

- `Call::Apply` は、省ける着地の印 `prompt: Option<VarId>` を持つ。印の変数 v は evidence (`obj`) の変数である。意味は「宛先が v の中断は、この呼び出しの結果にダミーを入れて、呼び出しの次から続ける」である (下の「実行の意味」、[VM](vm.md) の「巻き戻し」)。Core IR の `apply` も同じ欄を持ち、LIR への変換がそのまま写す
- テキストの形は、呼び出しの後に `prompt v` と書く (`let x.5: tobj poly = yields apply body.4(()) prompt ev.2`)。`mask` は呼び出しの前に書くので、印と並ぶことはない
- 印は呼び出しの使いではない。v を消費も読みもしないので、呼び出しが使う値 (`Call::for_each_atom`) にも、生存解析にも、Perceus と所有の検査の消費にも入らない。v は、続く `prompt$` の呼び出しが使うので、着地の後も生きている。巻き戻しは着地したフレームの値を手放さないので、v は着地の後も有効である
- 印を付けるのは EP だけで、付ける所は2つである。handle の `apply body(())` (v は handle の evidence) と、`resume$` の `apply κ(v)` (v は再開で作り直した evidence) である。どちらも直後に `prompt$` の呼び出しが続き、`prompt$` が `yield_status` の状態 2 の経路に進む ([Core IR](core-ir.md) の「handle と `prompt$`」と「再開と状態」)
- 印の付いた呼び出しは末尾の位置にない。直後に `prompt$` の呼び出しが続くので、縮約に印のための規則は要らず、verifier が確かめるだけにする (下の「verifier」)
- 印は `apply` にだけ付く。EP が印を付ける2か所が、どちらも `apply` だからである

### 入口の関数の印

- 関数は、in-place の入口の関数にだけ `(effect, op)` の印 (`clause`) を持つ。テキストの形では `internal yields clause State.get fn …` と書く
- 入口の関数は、捕獲の後に evidence と操作の引数を受け、操作の結果を返す。`ev_set_clause` に渡す値としてだけ参照され、`apply` でも直接の呼び出しでも呼ばれない

### ダミーの値

- yield 中に返す値は、呼び出し元が使わずに捨てる
- 関数の `ret` が `obj` か `tobj` なら `prim dummy()` の値を、`int` なら `0` を、`float` なら `0.0` を、`enum` なら `#0` を、`unit` なら `()` を返す。`tobj` にも物体を使うのは、互換の位置を通って `obj` の変数に入ることがあるためである (`ret` が `tobj` の関数の結果を `obj` の変数で受ける形)。`ret` が構造体なら、構造体の変数に `prim dummy()` を束縛して返す。box の挿入がこの束縛を `unbox` で受け直すので、開いた後は、下の構造体の `unbox` の約束で葉のダミーになる ([Core IR](core-ir.md) の「yield の地点」)
- ダミーは、変換と RC の命令に当たる。EP は遅い道の先頭で、使わない x を `drop` し ([Core IR](core-ir.md) の「yield の地点」)、box の挿入は呼び出しの結果を受け直す `unbox` と `box` を呼び出しの直後 (yield の確認の前) に置く。そのため、実行時は次を守る。`eml_rt` も同じ約束を守る
  - `dummy` の値は、`dup`、`decref`、`drop` で何も起こさない
  - `unbox` は、ダミーを受けても誤りにせず、何かの値を返す。その値は捨てられる
  - `box` は、ダミーを受けたらダミーを返す。`0`、`0.0`、`#0` は普通に箱に入れ、その値は捨てられる
  - 構造体の `unbox` は、ダミーを受けたら葉にダミーの値 (`prim dummy()` の値、`0`、`0.0`、`#0`) を書く
- 薄い形の版 (上の「関数の ABI」) の `call f` は yield の確認を持たないので、yield 中も結果を箱に入れて返す。スカラーのダミーは上の `box` の約束でダミーか捨てる箱になり、構造体の結果は葉のダミーを入れた配置の物体になる。この物体は yield の道で1回確保し、呼ぶ側が使わずに手放す
  - `release unboxed` は、ダミーを受けたら何もしない。名前を書いた葉は、すでにダミーの値である。token を定義する形は空の token を定義する
- 構造体の `unbox` の箱を手放す `release` にだけ印 (`unboxed`) を付けるのは、ふつうの `release` の速い道 (`rbtree` や `tree` の繰り返し) に、確かめの分岐を足さないためである。VM は別の命令にする ([VM](vm.md) の「データとクロージャ」)
- 中断が着地したときは、着地の印の付いた `apply` の結果に `prim dummy()` の値が入る。`apply` の結果はいつも `tobj` なので、Repr の違いは起きない。続く `prompt$` は、状態 2 の経路の先頭でこの値を `drop` する ([Core IR](core-ir.md) の「handle と `prompt$`」)
- 中断で外すフレームには値を返さない。`never` の節の入口は `yield_abort` の後にダミーを返す形をしているが、その `return` には実行が届かない。`IO` の extern が失敗して `io_failed` の節を呼んだときも、extern の行き先には何も書かない ([VM](vm.md) の「巻き戻し」)
- box の挿入が、yield の地点の結果の変換を確認の後に置く形は採らない。box の挿入に EP の形を知る特別な規則が要るためである
- yield の確認のない呼び出しの後に残る変換の扱いは、[Core IR](core-ir.md) の「yield の確認のない呼び出しの後」にある

### `evv` の不変条件

- 呼び出しが普通に返ったとき、`evv` は呼び出しの前と同じである。`evv` を替える所 (handle の `prompt$`、`resume$`、`mask` の包み、under の形の入口。[Core IR](core-ir.md) の「evidence passing」) が、戻るときに必ず元に戻すためである
- yield 中の `evv` には意味がない。再開のときに、包むフレームが外側から順に `evv` を作り直す
- 中断の途中の `evv` にも意味がない。巻き戻しは `evv` に触れず、着地した `prompt$` が `evv` を宛先の evidence の外側に戻す。`exit` では `Finish` が空に戻す ([VM](vm.md) の「extern の中断」)
- `mask` 付きの呼び出しは、戻りで `evv` を戻すのでフレームを残す。`mask` は見える別々の handler を飛ばすので、継続の上で `mask` が残すフレームは handler の数以下である ([エフェクトと handler](effects.md) の「健全性」の最後の段落)。`prompt$` と `resume$` のフレームは、handler 1つにつき1つである
- そのため、末尾呼び出しの保証 (上の「末尾の位置の呼び出し」) は、`mask` 付きの呼び出しを除いて保てる
- 実行が正しく終わったとき、`evv` は空で、yield の状態はなしである。VM はこれをモードによらず、検査モードのリークの検査の前に確かめる ([VM](vm.md) の「誤り、止めること、終わり」)

## パス

### パスの順番

- LIR のパスは、box の挿入、開く、縮約、Perceus、reuse の順である。`eml_lir::Pass` は `Boxing`、`Flatten`、`Contract`、`Perceus`、`Reuse` の5つで、`passes_until(&mut Program, Pass)` は、変換の直後の LIR に、box の挿入から指定したパスまでを順にかける。LIR のパスの順番を知るのはこの関数だけである。Core IR のパスと、2つの層をつなぐ順番は [Core IR](core-ir.md) の「パス」にある
- verifier はデバッグビルドだけでかけ、誤りはパスの名前を付けて報告する
  - `passes_until` は、最初に入力に入口の段 (`verify_lowered`) をかけ、誤りを `internal error: invalid LIR before boxing: {error}` と報告する。box の挿入の入力の約束であり、手で書いた LIR を `passes_until` に渡すテストもこの検査を通る
  - box の挿入の後は範囲の段 (`verify_scopes`) を、開くパスと縮約の後は開いた後の段 (`verify_flattened`) を、Perceus と reuse の後は所有の段 (`verify`) をかけ、誤りを `internal error: invalid LIR after {pass}: {error}` と報告する
- 5つのパスは、テストが直接呼べるように、それぞれ公開する (`boxing`、`flatten`、`contract`、`perceus`、`reuse`)
- box の挿入を開くパスより先に置くのは、一様な位置との変換と覗き穴を、構造体を1つの変数のまま今の規則で済ませるためである。開くパスを縮約より先に置くのは、開くパスが残す使われない読み出しを縮約が消すためである
- `eml_core_ir::lower` は reuse まで、`lower_until` は指定したパスまでかける

| パス | 受け取る IR | 渡す IR |
|---|---|---|
| box の挿入 (`boxing`) | 末尾呼び出し、`box` と `unbox`、RC の命令のない LIR | 位置の規則を満たす LIR。`split` 以降の一様な位置を一様にした関数と、版 `<f>$uniform` を含む |
| 開く (`flatten`) | box の挿入の後の LIR | 構造体の Repr のない LIR。構造体は葉の変数の並びになる |
| 縮約 (`contract`) | RC の命令のない LIR | 使われない純粋な `let` を消し、末尾呼び出しを作った LIR |
| Perceus | RC の命令のない LIR | `dup` / `decref` / `release` が入った LIR |
| reuse (`reuse`) | Perceus の後の、token の命令のない LIR | 手放す箱を token にし、同じ語の数の `con` がそれを使う LIR |

- どのパスの後でも、ブロックの列は上の「構造の規則」を満たす。パスの間で古くなる索引やキャッシュは IR に持たせない
- 借用パラメータの最適化はしない ([ロードマップ](../future/roadmap.md) の「段の列」)。再帰の深さの決まりは、[Core IR](core-ir.md) の「パス」にある

### box の挿入

- box の挿入 (`boxing`) は、プログラム全体を1回で扱う。関数の ABI を `split` で決め、値として参照する型どおりの関数の版を作ってから、位置の規則 (上の「位置の規則」) に合わない値に `box` と `unbox` を入れる
- 型は読まない。読むのは、呼ばれる関数のシグネチャ、`split` と `clause` の印、配置の表、extern の行、操作の表の `params` と `ret`、基本操作の表だけである
- 入力はエフェクトの命令を持たない (LIR の型にない)。`clause` は操作の表の `params` と `ret` を相手に、`prim` は基本操作の表を、`extern` は extern の行を相手に、直接の呼び出しと同じく変換を入れる。`clause` の先頭の evidence は `obj` である
- 構造体は箱を要する Repr として、スカラーと同じ手順に乗る。構造体を1つの変数のまま扱い、葉に開くのは次の開くパスである
- 縮約の前に置くので、パスは変換を位置ごとに素朴に入れればよい。使われなくなった `box` と `unbox` は、縮約がほかの使われない純粋な `let` と一緒に消す
- 手順は次の順である
  1. `split` による一様化: 各関数を、`split` が一様にする位置 (上の「関数の ABI」) で一様な Repr にする。互換でない引数を、同じ名前の新しい一様な Repr の引数に替え、入口のブロックの先頭で、引数の順に `let p = unbox p'` を置く。`ret` は一様な Repr にし、`return` の変換は 2. が入れる。`clause` の印の関数は、`split` でなく操作の ABI に合わせる。捕獲は一様な Repr、evidence は `obj`、残りの引数と `ret` は操作の表の Repr (`never` の操作の `ret` は `obj`) である。操作の表は構造体の型の位置を `obj` にするので、構造体の引数も入口で受け直す。入力に `tail`、`box`、`unbox`、RC の命令はない (入口の段の verifier が拒む)
  2. 版の形: 書き換える前のプログラムで、関数ごとの末尾の位置の呼び出しを集め、`needs_copy` を求める (上の「関数の ABI」)
  3. 版の作成と向け直し: 関数を順に見て、値の参照 (`&f` と `closure f(..)`) のうち、捕獲より後ろの引数と結果が一様でない関数を指すものを、その版に向ける。版がなければ作って関数の並びの末尾に足す。版も並びに足されるので、同じ順で見る
  4. 末尾の振り替え: 3. と同じ順で、結果が一様な関数 (薄い形の版を除く) の、結果が型どおりの関数への末尾の位置の直接の呼び出しを、その関数の版に向ける。3. と 4. は新しい版が出なくなるまで続く。デバッグビルドでは、版をそれぞれ元の関数から作り直し、同じ関数になることを確かめる。版が名前だけで決まることの確かめである
  5. 変換: 関数ごとに、ブロックを番号の順に、文を前から見て、各アトムを位置の Repr にする。`jump` と `return` は translate の出力では Repr が同じなので、変換が要るのは 1. と 3. で `ret` を変えた関数の `return` だけである
     - 互換でないアトムは、箱を要する Repr から一様な Repr へは `let b = box a` を、一様な Repr から箱を要する Repr へは `let v = unbox a` を、使う文の前に置いて渡す。下の覗き穴を先に試す
     - 位置の Repr は、直接の呼び出しなら呼ばれる関数の引数と `ret`、`closure g(..)` なら `g` の引数、`apply` なら呼ばれる値と引数と結果のどれも `tobj` である。`par` なら2つの子が `tobj` で、結果が `obj` である。組の構築子のフィールドは `tobj` なので、構造体の箱と同じ形である。そのため、`par` の結果を構造体の変数で束ねると、下の受け直しが `obj` の新しい変数で受け、構造体の `unbox` で取り出す。末尾の位置の呼び出しも同じ規則で変換する
     - 束縛の位置 (直接の呼び出し、`apply`、`clause`、`par`、`prim`、`extern` の結果、`unpack` と case のフィールド) で、受け取る Repr と変数の Repr が互換でなければ、受け取る Repr の新しい変数で受ける。元の変数は、その直後で `unbox` か `box` で定義する。case のフィールドなら、その定義を置く所は、元の変数を使うブロックすべての最も近い共通の支配者から始めて決める。変換を入れ終えた後で元の変数が残っている所は、どれも元の Repr を要する。`tobj` の位置へは、下の覗き穴が受け直した変数を渡すからである。行き先の先頭に置くと、元の Repr で使うのが後の1つの枝だけでも、行き先を通るたびに変換が走る。たとえば AA 木の `upsert` は、比べたキーが等しい枝でだけ値を `Int` として使うが、先頭に置くと節をたどるたびに `unbox` する。使う所がなければ定義を置かない。共通の支配者と行き先の間 (行き先を除き、共通の支配者を含む) に合流 (`jump` の行き先) があれば、そのうち最も行き先に近いものの直接の支配者に置く。なければ共通の支配者の先頭に置く。置く所は、行き先から `switch` の辺だけで届くブロックのうち、共通の支配者かその祖先で最も深いものになる。R3 から行き先へ入る辺はその `switch` の1本だけなので、使う所はどれも行き先に支配される。そのため置く所も行き先に支配されて受け直した変数はそこで定義済みであり、置いた定義は使いをすべて支配する (R6)。支配木は verifier と同じ辺の規則の検査 (`check::shape`) で求め、受け直す case のフィールドがある関数でだけ作る。`unbox` は読むだけだが、受け直した変数の寿命は置く所まで延び、借りた `tobj` のフィールドとして、下の Perceus の「所有を取る時点」に当たりうる。合流の前で止めるのは、その3. (`jump` の行き先の入口で生きている) を避けるためである。合流の入口で生きていると、入る `jump` ごとに `dup` が付き、合流で `decref` が要る。`switch` の辺だけで届くブロックへの道は1つなので、置く所までフィールドが `jump` をまたぐことはない。呼び出しの前では止めない。持ち主の最後の使いが呼び出しの前にあり、フィールドを呼び出しの後で使うと、その2. で持ち主を解く。持ち主が共有なら、行き先の先頭に置くより `dup` と `decref` が1つずつ増える。増えるのはその呼び出しを通る道で1回ずつで、使わない道で省く `unbox` と引き換えに受け入れる。`never` の操作の `clause` の束縛は受け直さず、その束縛を使う所にも変換を入れない (上の「`never` の操作」)。呼び出しの結果を受け直す `unbox` と `box` は、呼び出しの直後 (EP の yield の確認の前) に置く。yield 中はダミーに当たる (上の「ダミーの値」)
     - 使う所の変数は書き換えないので、R5 と R6 はそのまま成り立つ
     - 新しい変数の印は次のとおりである。スカラーの `box` と `unbox` で足す変数は `unr` にする。スカラーの印は型の Kind によらず `unr` だからである ([Core IR](core-ir.md) の「translate が決める印」)。構造体の `box` と `unbox` で足す変数と、受け直す新しい引数は、元の変数の印を写す。構造体は `lin` の値を持ちうるためである。印は変換の前後で変わらないので、誤った印を付けたパスの直後の段で V2 が見つける
     - 新しい変数は、元の変数の名前に、変数の表の末尾の番号を付けたものにする (`x.5: tobj`)。`Int` の定数を `box` した変数は `b` とする。受け直す新しい引数も、元の引数の名前を使う
  6. 型から起きない変換に当たったら、パスはパニックする。箱を要するスカラーと `obj` の間 (どちら向きも)、違う Repr のスカラーどうし、`unit` と箱を要する Repr の間、違う形の構造体どうし、構造体とスカラーの間である。translate の出力では起きないので、内部の誤りである
- 覗き穴: 変換を作る代わりに、定義をさかのぼって元の値を渡す
  - 箱を要する Repr の変数 v を一様な位置へ渡すとき、v が `let v = unbox w` で定義されていれば、`box` を作らずに w を渡す
  - 箱の変数 v を Repr が s の位置へ渡すとき、v が `let v = box w` で定義され、w の Repr が s なら、`unbox` を作らずに w を渡す。構造体の箱の `obj` にも当てる
  - 表は、パスが作る定義 (受け直しの `unbox` と `box`、一様にした関数の入口の `unbox`) からだけ作る。w は v の定義より前で定義されるので、v の使いをすべて支配し、R6 を保つ。使う所で作った変換は、使う所がほかの使いを支配しないので、同じ変数のほかの使いで使い回さない
  - 覗き穴がないと、恒等に近い関数 (handler の `return` の節、`$cont$` の転送) が値を往復させ、末尾呼び出しも失う。覗き穴があれば `return r'` になり、縮約が使われない `unbox` を消して `tail` を作る。Perceus から見ると、w の寿命が延びるだけである
- translate の転送が box の挿入より前にあるので、`return` だけの合流のブロックへ入る枝は、それぞれの `return` で `box` する
- パスはブロックと文のループだけで、IR の大きさに比例して再帰しない
- box の挿入より後に置くパスは、位置の規則を保たなければならない。呼び出しを作るか行き先を変えるパスの置き場所は、[Core IR](core-ir.md) の「パス」にある

### 開く

開くパス (`flatten`) は、構造体の変数を、形の葉の順 (深さ優先) に葉の変数の並びに替える。box の挿入が一様な位置との変換を構造体の変数のまま済ませた後に走るので、ここでは位置ごとの変換を決めない。

- 葉の変数の名前は元の変数の名前に葉の番号を付けた名前である。テキストの形では変数の番号も後ろに付くので、`d.3` の葉は `d.0.12`、`d.1.13` のように並ぶ。Repr は形の葉のもの、印は形の葉の印と変数の印の小さいほう (`Mark::meet`) にする。`optimize` の複製が開いた引数の印を小さくすることがあるためである ([Core IR](core-ir.md) の「`con` の値の形についての特殊化」)。`unit` の葉は変数を作らず、使う所を `()` にする
- 関数の引数、ブロックの引数、`jump` の実引数、直接の呼び出しの引数、`closure` の引数を、葉の並びに開く。`apply` の引数は box の挿入が `tobj` にしてあるので、開かない
- 構造体を束縛する `con` は消し、束縛した変数の葉を `con` の引数に置き換える。引数が定数なら、その葉は定数になる。構造体の `unpack` も消し、フィールドの変数を値の葉に置き換える。葉が定数になったことで定数で分ける `switch` ができれば、その行き先への `jump` にし、届かなくなったブロックを落とす
- `box s` は、配置の宣言したフィールドの Repr に合わせて葉を変換した `con L #0(..)` にする。`tobj` のフィールドならスカラーの葉を `box` し、入れ子の構造体のフィールドは再帰で箱にする。葉が、下の `unbox` が箱のフィールドから取り出した値なら、`box` せずにそのフィールドの語を渡す (葉の単位の覗き穴)。同じ関数の中で組の要素を入れ替えて箱に戻す形で、要素ごとに `Float` の箱を作らないためである
- `unbox w` は、構造体の `unbox` (`let (x.0, ..) = unbox w L`) にする。箱の `tobj` のフィールドからスカラーの葉や入れ子の構造体を得るときは、続けて `unbox` を置く。使わない葉は `_` にして読まない。フィールドから葉を取り出す `unbox` と入れ子の構造体の `unbox` は、使われなければこのパスが消す。最も外の構造体の `unbox` は、葉がどれも使われなくても残し、縮約に任せる (下の「縮約」)
- 直接の呼び出しの結果が構造体なら、複数の値の `let` で葉を束縛する。結果が1つの語の構造体なら、1つの値の `let` にする。`return` は、複数の値の `return` にする。関数の結果 `rets` は葉の並びになる
- 構造体の `drop` は、`unr` でない RC の対象の葉ごとの `drop` にする。結果の `unit` の変数は `()` に置き換える
- 戻らない呼び出し (`never` の操作の `clause`、戻らない extern) が構造体を束縛するときは、結果を `unr` の `obj` の変数 r で受け、葉をダミーの値で定義する。box の挿入は戻らない呼び出しの束縛の使いに変換を入れないので、1語の位置 (`tobj` のフィールド、`obj` の引数、`return` の1つの値) の使いが残りうる。開くパスはその使いを r に置き換える。制御が届かないので、どちらも実行されない
- 消した構造体の変数は、変数の表に、名前が空で Repr が `unit` で印が `unr` の項目として残す。変数の番号を変えないためである
- 開いた後の関数は、開く前の ABI を持たない。`apply` の引数と結果は一様な1語で、捕まえた値はクロージャの中で開いた葉の語のまま並ぶので、関数の値の ABI は開いた後の引数と `rets` だけで決まる (上の「関数の ABI」、[VM](vm.md) の「呼び出しと戻り」)
- 開いた後の LIR には構造体の Repr が残らない。extern、`prim`、`clause` の引数と結果には、box の挿入の後に構造体が来ない
- 構造体の値は葉の並びとして移る。`Lin` のフィールドを持つ構造体も `Lin` のまま移すので、`Lin` の葉は1回だけ消費される
- パスはブロックと文のループだけで、IR の大きさに比例して再帰しない。再帰は入れ子の構造体の形の深さ (語の数の上限で抑えられる) だけである

### 縮約

- 使われない純粋な `let` を消す。純粋なのは `const`、`con`、`closure`、`box`、`unbox`、`prim root_get`、`prim empty_mut_array` と、表の行が `Pure` の `extern` である。ただし、`Lin` の引数を借りずに受ける `Pure` の行 (`Std.MutArray.freeze`) は消さない (`FunctionRow::removable`)。消すと `Lin` の値を手放す命令がなくなり、Perceus がその変数を `decref` して V1 が拒むためである。呼び出し、`Panics` と `Effectful` の `extern`、`prim`、`drop` は、使われなくても消さない。ほかの基本操作は実行の文脈か evidence を読み書きするので、純粋でないとみなす。使われない `box` を消しても確保が1つ減るだけで、`unbox` は読むだけなので、どちらも評価の順を変えない
- 構造体の `unbox` も純粋で、使われない葉は `_` にして読まない。葉がどれも使われない構造体の `unbox` は、箱が `unr` か、箱が末尾の位置の呼び出しの結果で、`unbox` がその呼び出しの後ろにあるときだけ消す。後者は box の挿入の覗き穴が残すもので、消すと末尾の位置の呼び出しの形が保たれる。箱は呼び出しの結果として返るので、手放す所がない。ほかの箱の `unbox` は呼び出しの後ろにあっても残す。Perceus はこの文で箱が構造体の箱だと知り、`unr` でない箱を手放す所を、V1 が拒む `decref` でなく `drop` にする (下の「Perceus」)
- 縮約は、使われない純粋な `let` をすべて消し、いつも Perceus より前に走る。Perceus はこれに頼る。translate が開いた `unr` でない `con` の変数 ([Core IR](core-ir.md) の「translate が決める印」) は、使われない `con` として残り、その `lin` の引数は葉も読む。Perceus の入力にこの `con` が残ると、Perceus がその `lin` の `con` を `decref` し、V1 が拒む。そのため、Perceus の入力には、`unr` でない引数を持つ使われない `con` と `closure` がない
- ブロックを後ろから、文を後ろから見る1回のパスで、関数全体の使用の数を使う。定義は使う位置を支配し、辺は番号の大きいブロックへ向かうので、`let` を見る時点でその変数の使用はすべて見終わっている。消した `let` だけが使っていた束縛も同じパスで消えるので、1回で不動点に達する。デバッグビルドでは、2回目のパスが何も変えないことを確かめる
- 末尾呼び出しを作るのは縮約だけである。使われない `let` を消した後で、すべてのブロックに末尾呼び出しの規則を当てる。`let x = <呼び出し>` の直後の終端が `return x` なら、2つを `tail` にする。`let (x1, .., xn) = <呼び出し>` の直後の終端が同じ並びを同じ順に返す `return (x1, .., xn)` も同じである。呼び出しの結果の Repr は関数の `rets` と互換でなければならない。互換でなければ `tail` にしない (上の「末尾の位置の呼び出し」)。文を消したかに関わらずすべてのブロックを1回見るので、時間はブロックの数に比例する
  - 印には条件がある。印の受け渡しは推移的でないので、x を通してつながっていた結果の印と関数の戻り値の印を、値ごとに直接つないでも、結果の印を戻り値の印に渡せる (`Mark::passes_to`) ことを、`tail` にする条件にする。結果の印は、直接の呼び出しなら呼ばれる関数の戻り値の印、`apply` なら `poly` (いつも渡せる)、`clause` なら操作の表の戻り値の印である。`never` の操作の `clause` は結果を返さないので、どの戻り値の印にも渡せるとする。値の数が違う形 (構造体と箱の間) は `tail` にしない
  - 末尾呼び出しを変換を入れた後に作るので、Repr のための降格は要らない
- 縮約は、評価の順も短絡評価も変えない。消すのは実行時に何も起こさない右辺だけである

### 生存解析

- ブロックを後ろからたどる1回のループで、ブロックごとの入口の生存集合を求め、側の表として返す。IR には書かない。後ろからたどれば、行き先の入口の集合はいつも求め終わっている
- `switch` の終端の直前の生存集合 (終端が使う変数を含む) は、各 case の (行き先の入口 − case のフィールド) と `default` の行き先の入口の和に、scrutinee を足したものである
- 使いは「消費」と「読む」の両方を数える
- `jump` の終端の直前の生存集合 (終端が使う変数を含む) は、(行き先の入口 − 行き先の引数) に、実引数を足したものである
- 複数の値の `let` と構造体の `unbox` は、定義する変数の並びを1つの文の定義として扱う。規則は1つの変数を定義する文と同じである
- 生存集合はすべての変数を持つ。Perceus は RC の対象だけを見るが、VM のスロットの割り当ては RC の対象でない変数も干渉に数えるためである ([VM](vm.md) の「スロットの割り当て」)
- EP は、同じ規則の Core IR の上の写しを使う ([Core IR](core-ir.md) の「継続の関数」)
- 文の単位の生存も、`eml_lir::liveness` の公開の API にする。ブロックの入口の生存集合 `live_in`、終端の後で生きている変数の集合 `live_after_term` (行き先の入口の生存集合から行き先のブロックの引数、`switch` では case のフィールドを除いて合わせたもの。終端が使う変数も、行き先で生きていれば入る)、生存集合を文の前へ1つさかのぼる `step_back` である。VM のスロットの割り当てが、文の単位の干渉を作るのに使う ([VM](vm.md) の「スロットの割り当て」)。`step_back` は `dup`、`decref`、`release` も使いとして数えるので、Perceus の後の LIR にも使える

### Perceus

Perceus は、`switch` と `unpack` のフィールドを借りたまま使い、所有が要る所でだけ所有を取る。持ち主が要ればフィールドを `dup` し、要らなければ持ち主を `release` で解いてフィールドを受け取る。そのため、読むだけのフィールドには RC の命令が付かず、分解した後に使わない持ち主のフィールドは `dup` されない。`Lin` のフィールドが `dup` されないのは、この形による (上の「値の表現」)。

- EP と translate が出した `drop` は消費なので、Perceus はそこに何も足さない。Perceus が `unr` でない変数に出す命令は、分解のときの `release` と、`unr` でない構造体の箱を手放す `drop` だけになる (下の「verifier」の V1)
- 入力は RC の命令を持たない。ブロックを番号の順に1回たどる。各ブロックでは、後ろからたどって各命令の後の生存を求め、前から状態 (下の「経路ごとの状態」) を持って命令を足す
- EP の出力のための規則は足さない。`prim` の借用の引数は読む使いで、ほかの引数は extern と同じく消費する。結果は所有した値である (上の「基本操作」)。`clause` は呼び出しとして扱う
- 変数を消費することは所有権の移動である。読む使い (上の「ブロックの列」) は所有権を動かさない
- タグの case を持つのに配置のない `switch` を見たら、Perceus はパニックする。verifier が拒む形なので、黙って飛ばさない
- 構造体の `unbox` の葉は、`unpack` のフィールドと同じく箱から借りる。親とスロットも `unpack` と同じに決める
- 構造体の `unbox` が読む箱 w を手放す所は、次のようにする
  - 属する生きている葉がなく、w の印が `unr` でなければ、`decref` (V1 が拒む) でなく `let u = drop w` にする。`Lin` の箱を手放すためである。w の印が `unr` なら、`decref` にする
  - 葉を受け取るために w を解く `release` には、w の印によらず `unboxed` の印を付ける (`release unboxed w L #0(..)`)。w がダミーでありうるためである (上の「ダミーの値」)

#### 親、スロット、持ち主

- 親とスロット: `switch` の case と `unpack` が束縛したフィールドの変数の、分解した値 (親) と、フィールドの位置 (スロット) である
- 持ち主: verifier の持ち主と同じ規則で、束縛の時点で決める (下の「verifier」)。親が束縛の時点で所有されていれば親、そうでなければ親の持ち主である。後で親が所有になっても、持ち主は変わらない
- o に属する変数: 持ち主が o で、その位置で借りている (所有の数が0の) 変数である
- 命令で必要な変数: その命令の後で生きている変数と、その命令が使う (消費するか読む) 変数である
- 変数は関数の中で1回だけ定義される (R5) ので、親、スロット、持ち主は、verifier と同じく関数全体の表に持つ

#### 経路ごとの状態

- 経路ごとに持つのは、所有している変数の集合と、持ち主ごとのそれに属する生きている変数だけである
- 保持: 所有していて生きていないが、生きている変数が属している変数を、保持している変数と呼ぶ。下の「持ち主を手放す時点」で手放す
- 関数の入口と合流するブロックは、入口で生きている RC の対象の変数と RC の対象の引数をすべて所有して始まる。借りた変数はない
- `switch` の行き先は、`switch` のブロックを前からたどり終えた状態を引き継ぐ。case のフィールドのうち、行き先の入口で生きている RC の対象が、借りた変数として加わる。`default` の行き先は、その状態をそのまま引き継ぐ

#### 所有を取る時点

借りた変数 v の所有を取るのは、次のどれかに当たるときだけである。

1. v を消費する
2. v が呼び出し (`call`、`apply`、`clause`) の後で生きていて、v の持ち主 o がその呼び出しで必要でない (o が保持されている)。o が呼び出しの後でも生きているなら、v は借りたまま呼び出しをまたぐ。o はそこまで生きているので、v も有効なままである。呼び出しが o の最後の参照を消費するなら、4. で所有を取る
3. v が `jump` の行き先の入口で生きている。合流するブロックは所有だけで始まるためである
4. v の持ち主 o の最後の所有の参照を、ある命令が消費し (o がその命令の後で生きていない)、v がその命令の後で生きている。たとえば `| Box s -> (Some b, s)` の `con Option #1(b)` は、b を消費した後に s を使う

`switch` の scrutinee、`unpack` の値、`unbox` のオペランド、`prim` の借用の引数は読む使いなので、借りた変数のまま使う。

所有の取り方は、v の持ち主 o で決める。1つの命令で所有を取る変数が複数あれば、番号の順に取る。

- o がその命令で必要なら、命令の前に `dup v` を置く。o はまだ所有されているので、v は有効である。4. はいつもこの場合である
- o が必要でない (保持されている) なら、命令の前で o を解く (下の「持ち主を解く」)。o に属する生きている変数は、まとめて所有になる。2. はいつもこの場合である

1つの命令の前では、所有を取る命令 (`dup` と、持ち主を解く命令) を先に置き、消費の数で数える `dup` をその後に置く。消費の数の `dup` は、同じ変数を2回以上消費するときと、消費した後も生きているときと、同じ命令が借りているときに要る。所有を取った後の変数は、ほかの所有の変数と同じに扱う。

#### 持ち主を解く

保持されている o を解くときは、次の順に命令を置く。

1. o に属する生きている変数の集合を N とする。どれもその命令で必要である。命令の前で生きている変数は、命令の後で生きているか、命令が使う変数だからである
2. 親をたどって N の変数から o に着くまでの道にある各スロットに、名前を書く代表の変数を1つ決める。そのスロットを束縛した道の上の変数のうち、番号のいちばん小さい変数である。同じスロットを2つの変数が束縛するのは、同じ値を2回分解する形 (射影の繰り返し、`p.name ++ p.name`) である。親ごとに、道の上でその親を分解して束縛した変数のうち、番号のいちばん小さい変数のコンストラクタを、その親の `release` のコンストラクタにする。親をこれと別のコンストラクタで分解した変数 (到達しない行き先にしか現れない) は、1つの `release` に書けないので代表にしない
3. N の変数 g が次のどちらかに当たれば、`dup g` を置く。o はまだ所有されているので、g は有効である
   - g から親をたどって o に着くまでの間に、N の変数か、今所有している変数がある。間の変数はこの後も必要か、すでに所有されているので、`release` でその変数から受け取れない
   - g か、g から o までの間の変数に、代表でない変数がある。`release` に名前を書けないためである
4. `release o L #t(..)` を置く。各スロットに書くのは、代表が N に入るか、代表の子孫に 3. で `dup` していない N の変数があるときの、その代表である。名前を書くスロットがなければ `decref o` にする
5. 4. で名前を書いた代表のうち N に入らないもの (子孫のためだけに受け取ったもの) について、同じく 4. を当てる。親から子の順に、N の変数がすべて所有になるまで続ける

親から順に手放すのは、上の物体を先に一意にすると、`release` が一意の側を通り、子の参照を増やさずに受け取れるためである。

手順 3. の「今所有している変数」は、その時点で所有している変数である。前に所有になってから消費した間の変数は、今は所有していないので、この条件に当たらない。4. か 5. の `release` は、その変数に参照をもう1つ渡し、5. のその変数の `release` がそれを手放すので、釣り合う。

#### 持ち主を手放す時点

- 所有していて生きていない変数は、属している生きている変数がなければ `decref` し、あれば保持する。確かめる時点は次のとおりである
  - ブロックの入口: 変数の番号の順に `decref` する。`switch` の行き先でも同じで、フィールドを使わない scrutinee、ほかの行き先だけが使う変数、使わない引数がこれに当たる。scrutinee が借りた変数のときは、その持ち主が保持の対象になる
  - 各命令の直後: その命令で死んだ所有の変数 (定義して使わない変数と、`unbox` のオペランドや `prim` の借用の引数のように、読む使いの後に死ぬ値を含む)
  - 保持されている o に属する生きている変数が、最後の1つまで死んだ命令の直後: `decref o`
- `jump` の前: 上の「所有を取る時点」の 3. のとおり、行き先で生きている借りた変数をすべて所有にした後で、行き先で生きていない所有の変数 (保持されている変数を含む) を `decref` する。合流するブロックに入るすべての辺で、所有の集合が入口の生存集合のうちの RC の対象と一致する
- `return` と `tail` の前: 実引数の所有を取った後で、保持されている変数をすべて `decref` し、所有を残さない
- 呼び出しの前: 上の「所有を取る時点」の 2. のとおり、保持されている持ち主を解くので、保持されている変数は呼び出しをまたがない。保持されている持ち主が呼び出しをまたぐと、その死んだ兄弟のフィールドも残り、末尾でない再帰では段ごとに溜まるためである
- 終端を、文と新しい終端に置き換える編集をその場でできる形にしておく。借用パラメータの最適化 ([ロードマップ](../future/roadmap.md) の「段の列」) が、末尾の位置の呼び出しの輪の上にない `TailCall` を降格できるようにするためである (上の「末尾の位置の呼び出し」)

#### Perceus の時間

- 親、スロット、持ち主は関数全体の表に持つので、経路ごとの状態は、所有の集合と、持ち主ごとの属する変数だけである。属する変数を持ち主から引けるようにするのは、生存集合から探すと命令ごとの手間が生存集合の大きさに比例し、全体で2乗の時間になるためである
- `switch` の行き先へ状態を写す時間は、辺の数と状態の大きさの積に比例する
- 「持ち主を解く」は o に属する変数とその親の道をたどるので、時間は o に属する変数の数と分解の入れ子の深さの積で抑えられる
- 長い借用の `switch` の連なり、多くの生きている引数、1つの持ち主の多くの読み、1つずつ解く多くの持ち主で、時間が線形であることを `crates/eml_lir/tests/perceus.rs` のテストで確かめる

### reuse

reuse のパスは、Perceus が手放す箱を token にし、同じ物理的な語の数の `con` がその箱を使うようにする。箱が一意なら、解放と確保をしないで済む。どの `con` がどの token を使うか、ヘッダを書くか、どの引数を書かないか、使わない道のどこで token を手放すかは、すべて LIR に書く。VM とネイティブのコード生成は、書かれたとおりに `eml_rt` の部品を並べるだけである ([オブジェクトモデル](object-model.md) の「reuse」、[VM](vm.md) の「reuse」)。

#### token と命令

- token は、Repr が `token` で印が `unr` の変数である。RC の対象ではなく、`dup`、`decref`、`release` の相手にならない。extern の行、配置、操作の表には現れない。ネイティブのコード生成では、箱を指すか空を表す1語になる
- `let r: token = release x L #t(p1, .., pn)` は、`release` と同じく x を手放し、token `r` を定義する。x が一意なら、`_` の位置のフィールドを `decref` して、x の箱を token にする。そうでなければ `release` と同じに振る舞い、空の token を定義する。名前を1つも書かない `release x L #t(_, .., _)` は、token を定義する形でだけ書ける
- `let d = con L #k(a1, .., an) reuse r` は、token `r` を受け取る `con` である。token が空なら、普通の `con` と同じく確保する。空でなければ、その箱にヘッダを書き直して作る。`reuse r in place` と書いた `con` は、ヘッダを書かない。引数の前の `=` (`con L #k(=a1, a2) reuse r`) は、その引数を書かない印である。token が空のときは、書かない印の引数も書く
- `drop_token r` は、使わなかった token を手放す文である
- テキストの形は `eml_lir::parse` と `pretty` の両方で扱う

#### token の流れ

- token は、定義した文から後ろへ、同じブロックの中と、`switch` の行き先へ流れる。R3 から、行き先に入る辺は `switch` の辺1本である
- token は合流するブロックへ流れない。`jump`、`return`、`tail` の前で、まだ使っていない token は消える。合流で token を受けるには、道によって空の token を作る定数とブロックの引数の検査が要るためである
- yield しうる呼び出しの前でも、token は消える。継続を2回再開すると、yield の後の `con` が同じ箱を2回使うためである。yield しうる呼び出しは `eml_lir::reuse::may_yield` が決める。`yields` の関数の中の、`yields` の関数の直接の呼び出し、`yields` の `apply`、再開できる操作の `clause` で、EP の yield の地点に合わせている ([Core IR](core-ir.md) の「yield の地点」)。yield しない呼び出し、`Effectful` の extern、`never` の操作の `clause` はまたいでよい
- token は値ではない。呼び出し、`apply`、`clause`、extern、`prim` の引数、`closure` の捕獲、`con` の引数、`jump` の実引数、`return` の値、`switch` の scrutinee にならない
- 呼び出しをまたいで生きている token は、その呼び出しを通る中断の巻き戻しで手放す。`eml_lir::unwind::UnwindPoint` の `tokens` が、地点の直後に生きている token を並べる ([VM](vm.md) の「巻き戻し」)

#### reuse の元

- 元は、Perceus の `release x L #t(..)` と、構築子がその道で分かっている x の `decref x` である。レコードの更新では、フィールドがみな借りたまま読まれるので、Perceus は `release` でなく `decref` を出す。`decref x` は、`release x L #t(_, .., _)` に書き換えてから token を定義する
- 構築子が分かるのは、x を scrutinee とするタグの `switch` の case の行き先と、`unpack x` と構造体の `unbox` の箱 x の後である。合流するブロックへは、入るすべての道で分かっている構築子だけを渡す
- x の Repr は `obj` か `tobj` で、構築子はフィールドを持つ。フィールドのない構築子の値は、箱でなく即値である
- 組にする語の数は、構築子の `Shape::reuse_words` (走査する語と生の語の数の和) である ([オブジェクトモデル](object-model.md) の「ペイロード」)。走査する語が 255 個以上の構築子は、`eml_rt` だけが知る数の語を持つので、元にも `con` の相手にもならない
- 構造体の `unbox` の箱の `decref` から作る token の `release` にも、Perceus の `release` と同じく `unboxed` の印を付ける。箱がダミーなら空の token を作るためである。`unr` でない箱を Perceus が `drop` で手放す所は、元にしない
- translate と EP が出す `drop x` は元にしない。出る所が少なく、束縛が `()` の右辺を token の定義に変える手間に見合わないためである

#### 組の選び方

- 関数ごとに、ブロックを番号の順に1回たどる。道ごとに、構築子が分かっている変数と、使える token の並びを持つ。元の文で token を足し、`con` で同じ語の数の token を1つ取る。`switch` の行き先は、`switch` のブロックをたどり終えた状態を引き継ぐ。合流するブロックは token なしで始まる
- 1つの `con` に使える token が複数あるときは、書かない引数が最も多い token を選ぶ。同じ数なら、その場で作り直せる token を先に選ぶ。それでも決まらなければ、最も後に定義した token を選ぶ。`con` は文の順に1つずつ決める。`bench/upsert.em` の `split` を融合した形では、3つの `release` の後に3つの `con` が続く。この規則なら、書かない引数は順に 4 個、5 個 (すべて)、2 個になる。最も後に定義した token を先に選ぶ規則では、最初の `con` に書かない引数がなくなる
- Perceus は、保持した持ち主を、借りたフィールドが要る所 (末尾呼び出しの引数など) で解く。そのため、std の `List.map` のループのように、古いフィールドを読むだけで新しい `con` を作る形では、`release` が `con` の後に来る。そこでパスは、元の文を見つけたとき、同じブロックを元の文から前へたどり、元の文を越えて上げられない最初の文で止まる。越えられないのは、元の値を使う文、元の値を分解して束縛した変数を使う文、yield しうる呼び出しである。ただし、名前を書いて残すフィールドそのものは `release` の後に所有になるので、間で読んでも消費してもよい。残すフィールドをさらに分解した変数は、持ち主が元の値のままなので使えない (下の「verifier」)。`decref` の元は名前を書かないので、どのフィールドも残さない。止まった文より後ろにあって、まだ token を受けていない同じ語の数の `con` のうち、いちばん前のものの直前へ元の文を上げ、その `con` とすぐに組にする。そうした `con` がなければ上げない。そのため、組にならない元が動くことはない。いちばん前の `con` を選ぶので、後ろの `con` と組めば書かずに済んだ引数を書くことがある。reuse の回数は変わらない
- その場で作り直す (`in place`) のは、token の元と `con` が同じ配置の同じタグで、元の変数の印が `unr` か、`con` の束縛の印が `lin` のときである ([オブジェクトモデル](object-model.md) の「`lin` ビット」)。そうでなければヘッダを書き直す
- 引数を書かないのは、引数が token の元の値を `switch` か `unpack` で分解して束縛した変数で、元の構築子と `con` の構築子で、語の種類 (走査する語か生の語か) と物理的な位置が同じときである。箱は中身を消さないので、その位置には同じ語が残っている。配置が違う `con` でも、この条件を満たす引数は書かない
- どの道でも使わなかった token の元は、元の文 (`release` か `decref`) のまま残す。`into_token` と `drop_token` の2回の操作が、箱を解放する1回より安いことはないためである
- 一部の道でだけ使う token は、使わないと決まる最も早い位置で手放す。token は `switch` の辺でしか流れず、`jump`、`return`、`tail`、yield しうる呼び出しの前で消えるので、その位置は `switch` の行き先の入口である。行き先 t から `switch` の辺だけでたどれるブロックのどこでも token を使わないとき、t の入口の先頭に `drop_token` を置く。t より上の行き先の入口で手放した token は、t では手放さない
- 新しい token の変数は、名前を `r` にして変数の表の後ろに足す
- パスが読むのは、その関数の LIR、呼ぶ関数の `yields` の印、配置の表、操作の表だけである。関数のコードはその関数の名前で決まる ([VM](vm.md) の「追記」)

### verifier

- LIR の verifier は5つの段を持つ。段はパスの順に直線に並ぶので、境界の検査だけを切り替える別の旗は持たない
  - 入口の段 (`verify_lowered`) は、`passes_until` が入力にかける (上の「パスの順番」)。構造の規則のうち境界と所有に関わらないもの (R1〜R5、R6 の支配、R7 と R8 のうち境界の検査を除くもの) と、下の引き継ぐ検査と、基本操作と `clause` の検査を確かめ、`tail`、`box`、`unbox`、`dup`、`decref`、`release` がないことを確かめる。box の挿入は末尾呼び出しと変換のない入力を前提にし、`let` と `return` の形から末尾の位置を見るためである。Perceus より前に `release` を拒むのと同じく、パスの入力の約束を verifier が確かめる
  - 範囲の段 (`verify_scopes`) は box の挿入の後にかける。入口の段から `box` と `unbox` の拒否を除き、`box` と `unbox` の規則 (上の「値の表現」) と境界の検査を足す。構造体の変数を受け、複数の値の `let` と `return`、構造体の `unbox` を拒む。互換でない `return` は、末尾の位置の呼び出しの結果を返す形のときだけ許す (上の「末尾の位置の呼び出し」)
  - 開いた後の段 (`verify_flattened`) は開くパスの後にかける。範囲の段に加えて、変数、関数の結果、配置のフィールドのどこにも構造体が残らないことを確かめ、複数の値の `let` と `return`、構造体の `unbox` を受ける
  - 所有の段 (`verify`) は Perceus と reuse の後にかけ、開いた後の段に加えて、RC の対象の所有の多重集合 (R6)、V1、token の規則、`release unboxed` の印を確かめる
  - 入口の段のほかの4つの段を、境界の段と呼ぶ。境界の検査 (下) は境界の段で走る
- Kind の印の検査 (V1、V2、作る所の検査) は、下の「Kind の印の検査」にある。V2 と作る所の検査は Repr を比べる所と同じ所で、V1 は所有の段で走る
- 着地の印 (上の「着地の印」) の検査は、5つの段のすべてで走る
  - 末尾呼び出しの印は、呼び出しの検査の最初に拒む (``a tail apply has a prompt marker, but a tail call leaves no frame to land on``)。入口の段は `tail` そのものを拒むので、この文言は入口の段のほかで出る
  - ほかの印は、呼び出しの検査 (範囲と所有を含む) の後に確かめる。印の変数 v が見えること、v の Repr が `obj` であること、v が呼び出しの直後で生きていることを、この順に確かめる (`` `ev.1` is the prompt marker of an apply, but it is tobj, not obj ``、`` `ev.1` is the prompt marker of an apply, but it is not used after the call ``)
  - 所有の段では、v が呼び出しの直後にも所有されていることも確かめる (`` `ev.1` is the prompt marker of an apply, but it is not owned after the call ``)。巻き戻しは着地したフレームの値を手放さないので、続く `prompt$` が使う v は、着地の後も有効でなければならない
- `eml_lir::unwind::points` は、所有の段の走査に、巻き戻しの地点ごとの所有を書き留めさせる。VM のコード生成が、これで巻き戻しの表を作る ([VM](vm.md) の「巻き戻し」)。所有を求める走査を、verifier と巻き戻しの表で2つ持たないためである
- Core IR の verifier の2つの段は、印のない構造の規則と同じ部品 (`eml_lir::check`) を使う ([Core IR](core-ir.md) の「verifier」)
- 境界の検査は、R7 と R8 の検査のうち、`jump`、`return`、`box`、`unbox` を除くものである。直接の呼び出しの引数と結果、`closure` の引数、`apply` の呼ばれる値と引数と結果 (`tobj` と比べる)、`par` の子 (`tobj` と比べる) と結果 (`obj`)、`clause` と `prim` のオペランドと結果、`clause` の印の関数の引数と `ret` が操作の表と互換であること、宣言した Repr が `tobj` のフィールドである。`tail` の結果は、呼び出し元の `rets` と Repr と印を比べる。translate と EP の出力は、box の挿入より前には境界に合わないので、Core IR の verifier と LIR の入口の段は境界を見ない
- 所有は、どの経路でも、関数の入口と束縛で得た参照と、`dup` と `release` で増やした参照が、消費と `decref` と `release` でちょうど使い切られることを確かめる。経路ごとに持つのは、RC の対象の変数ごとの所有の数だけである。`return` と `tail` の時点では、所有が残らない
- RC の対象のフィールドには、定義のとき (R5 で1回だけ) 持ち主と出どころを記録する
  - 持ち主: 分解した値 s が、束縛の時点で所有の数を1つ以上持っていれば s、そうでなければ s の持ち主
  - 出どころ: `(s, Ctor, 位置)`。case の `Ctor` は、`switch` の配置と case のタグである。フィールドの数は R8 が配置で確かめるので、出どころに持たない
- 変数 v が有効なのは、v の所有の数が1つ以上か、v の持ち主の所有の数が1つ以上のときである。1回の参照で決まるので、verifier は線形のままである。所有の数が正なら、その経路は実際の参照を持ち、物体は生きている。データは変更されないので、その下の物体もすべて生きている
- 所有の段では、次の規則を確かめる
  - 分解 (case のフィールド、`unpack`): 値は見えて有効な RC の対象の変数である。RC の対象のフィールドは所有の数0で始まる。借りた変数への `switch` と `unpack` も受け入れる
  - 読む (`switch` の scrutinee、`unpack` の値、`unbox` のオペランド、`prim` と extern の借用の引数、`dup` の対象): 見えて有効である。所有の数を変えない。ただし `dup v` は v の所有の数を1つ増やす。`prim` の借用の引数が変数でない値なら、消費と同じ検査 (関数の値の番号) だけを通す。借りる変数を同じ命令が消費するなら、所有の数が消費の数より多くなければならない
  - 消費と `decref v`: 見えて、v の所有の数が1つ以上である。1つ減らす。1つの命令の中では左から順に当てる
  - `release x L #t(p1, .., pn)`: 名前が1つ以上あるか、token を定義する。x の所有の数が1つ以上である。名前を書いた各 pi は見えて、Repr が RC の対象で、出どころが `(x, L #t, i)` である。その後、x を1つ減らし、各 pi を1つ増やす。1つの位置には1つの名前しか書けないので、同じフィールドを2つの変数が引き継ぐことはない
  - 名前のない `release` では、その道で x の構築子が `L #t` だと分かっている。箱の子は `L #t` の形で手放され、token の語の数も `L #t` で決まるので、違う構築子なら fast mode で記憶がおかしくなるためである。分かっている構築子は reuse のパスと同じく道ごとに持つ。タグの case の行き先はその scrutinee の、`unpack` はその値の構築子を知り、合流するブロックには入るすべての道で分かっているものだけが残る。フィールドのない構築子は数えない。名前のある `release` には求めない。出どころが x を `L #t` として分解したことを示すうえ、両立しない2つの `switch` を通る死んだ道では、分かっている構築子が出どころと食い違うことがあるためである
  - token: token の変数は、token を定義する `release` でだけ定義する。経路でまだ使っていない token は所有の数1として持ち、所有と同じ写し方で `switch` の行き先へ渡す。どの経路でも、token を受ける `con` か `drop_token` で、ちょうど1回使う。token は値の位置 (上の「token の流れ」) に現れず、`jump`、`return`、`tail` の時点と、yield しうる呼び出しの直後に生きていない
  - token を受ける `con`: 物理的な語の数は token の元の構築子と同じである。`in place` なら、元と同じ配置の同じタグで、`lin` ビットの条件を満たす。書かない印の数は引数の数と同じで、書かない引数は、元の値を分解して束縛した変数で、語の種類と物理的な位置が元と同じである
  - 借りた変数 (所有の数0) は消費できないので、`jump` で渡されることはない。呼び出しの後も見えたままで、持ち主が所有されていれば有効である。持ち主を呼び出しの引数として消費すると、その後の借りた変数の使いは拒まれる。定義が支配する合流のブロックでも見えたままで、持ち主がどの辺でも所有されていれば有効である
  - 持ち主を手放した後は、間の変数を後で `dup` していても、借りた変数を拒む。保守的だが健全で、Perceus はその形を出さない。親をたどる規則に緩めても、IR は変わらない
- 引数のない `con` は、2つの IR のどの段でも拒む (`` a constructor value without fields is written as a tag `#N`, not `con` ``)
- 入口の段だけの文言は次のとおりである
  - `tail`: ``a tail call is formed before contract``。`call` のほかは ``a tail apply``、``a tail clause``、``a tail par`` を同じ形で使う
  - `box` と `unbox`: `` `n.1` is boxed before the boxing pass ``、``5 is boxed before the boxing pass``、`` `b.2` is unboxed before the boxing pass ``
- 入口の段と範囲の段の、開くパスより前を確かめる文言は次のとおりである
  - 複数の値: ``a let of 2 values is formed before flatten``、``a return of 2 values is formed before flatten``、`` an unbox of `w.3` as `Vec3` is formed before flatten ``
  - 関数の結果: ``the function returns (float, float) before flatten``
- 開いた後の段と所有の段の文言は次のとおりである
  - 残った構造体: `` `d.3` is Vec3{float, float, float}, but no struct remains after flatten ``、``the function returns Vec3{float, float, float}, but no struct remains after flatten``、``the constructor `Ball` has a struct field, but no struct remains after flatten``
  - 複数の値: ``a return gives 3 values, but the function returns (float, float)``、``2 values are bound to `f`, which returns (float, float, float)``
  - 構造体の `unbox`: `` `w.3` (float) is unboxed as a struct, but only obj and tobj can be ``、`` an unbox of `w.3` names `Shape`, which has 2 constructors ``
- 所有の段の `release unboxed` の文言は次のとおりである。`` `w.3` is unboxed as a struct, but its release is not marked unboxed ``、`` `w.3` is released as unboxed, but it is not unboxed as a struct ``
- 入口の段と範囲の段の文言は次のとおりである。Perceus より前の RC の命令を拒む
  - `` `s.0` is duplicated before Perceus ``、`` `s.0` is released before Perceus `` (`decref`)、`` `d.0` is released with its fields before Perceus `` (`release`)。token の命令も同じく拒む
- 所有の段の誤りの文言は次のとおりである
  - `` `x.1` is {used|released} but is only borrowed from `d.0` ``: 持ち主が所有されている間に、所有の数0の変数を消費したか、`decref` か `release` で手放した
  - `` `x.1` is {duplicated|switched on|unpacked|unboxed|lent to `prim ev_evv`} after its owner `d.0` was given up ``: 無効な変数を読んだ。`prim` の借用の引数では、基本操作をテキストの形 (即値を含む) で書く。extern の借用の引数では、行の名前を書く (``lent to `Std.Array.get` ``)
  - `` `x.1` is lent to `Std.Array.get` and consumed by the same instruction ``: 借りる変数を同じ命令が消費するのに、所有の数が消費の数以下である。`prim` の借用の引数では、基本操作をテキストの形で書く (``lent to `prim ev_renew` ``)
  - `` `x.1` is not field 0 of `d.0` as `Option` #1 ``: `release` の名前の出どころが違う
  - `` a release of `d.0` keeps no field `` と `` `x.1` is kept but is not reference counted ``: `release` の名前の誤り
  - `` `x.0` is released as `P` #0, but its constructor is not known on this path ``: 名前のない `release` の構築子が、その道で分かっていない
  - token: `` the token `r.4` is used twice ``、`` the token `r.4` is used as a value ``、`` the token `r.4` is still live at a return `` (`a tail call`、``a jump to b1`` も同じ形)、`` the token `r.3` is live after `g`, which may yield ``
  - token を受ける `con`: ``a con of `Q` #0 has 3 words, but the token `r.4` holds 2 words``、``a con of `Q` #0 rebuilds the token `r.4` in place, but the token comes from `P` #0``、``a con of `P` #0 rebuilds the token `r.3` in place, but `x.0` is lin and `d.4` is poly``、``argument 0 of a con of `P` #0 is skipped, but `b.3` is not at the same position in the box of the token `r.4` ``
  - 持ち主のない変数を読むか消費するか手放したときと、持ち主も手放した変数を消費するか手放したときは、`` `x.1` is {used|released|duplicated|switched on|unpacked|unboxed|lent to `prim ev_evv`} after it was moved `` で報告する
- R7 と R8 のうち境界の検査でないものは、2つの IR のすべての段で走り、どの段でも次の文言で報告する (`release` は LIR の所有の段にだけ現れる)。境界の検査と、`box` と `unbox` の規則は、境界の段で走る。どの文言の後にも、ほかの誤りと同じく `` in `f` `` が付く
  - 表にない配置: ``a con refers to the unknown layout #3`` (`a switch`、`an unpack`、`a release` も同じ形)
  - `switch` の配置の有無: `a switch with tag cases has no layout`、``a switch without tag cases has the layout `Prelude.Bool` ``
  - タグの範囲: ``a con names #2, but `Prelude.Bool` has 2 constructors`` (`a case`、`an unpack`、`a release` も同じ形)
  - フィールドの数: ``a con of `Option` #1 has 2 fields, but the constructor has 1``、``a case of `Option` #1 has 0 fields, but the constructor has 1``、``an unpack of `p.0` as `Pair` #0 has 1 fields, but the constructor has 2``、``a release of `d.0` as `Option` #1 has 2 fields, but the constructor has 1``
  - タグの `switch` の scrutinee: `` `c.1` (int) is switched on as `Prelude.Bool`, which is enum ``、``a switch on `Option` has the constant #1 as its scrutinee``
  - 網羅: ``a switch on `Option` has no default and no case for #0``
  - リテラルの `switch` の scrutinee: `` `s.0` (obj) is switched on Int literals ``、`` `n.0` (int) is switched on String literals ``、``() is switched on Int literals``
  - `unpack` の配置: `` `p.0` (obj) is unpacked as `U`, which is enum ``、``an unpack of `s.0` names `Shape`, which has 2 constructors``
  - `con` の束縛: `` `d.1` (obj) is bound to a con of `Option`, which is tobj ``
  - `tobj` でないフィールド: ``field 1 of `Pair` #0 is `n.2` (obj), but the layout has int`` (case と `unpack` のフィールド)、``argument 1 of a con of `Pair` #0 is (), but the layout has int``
- 2つの IR のすべての段の `jump` と `return` の文言は、互換で比べる。定数では ``5 is returned from a function that returns tobj``、``() is returned from a function that returns obj`` になる。変数の `` `x.1` (int) is returned from a function that returns obj `` と同じ形である
- 境界の検査の文言は次のとおりである
  - 直接の呼び出しの引数: ``argument 0 of `g` is `x.1` (int), but the function takes tobj``、``argument 1 of `g` is 5, but the function takes tobj``
  - 直接の呼び出しの結果: `` `t.0` (obj) is bound to `k`, which returns int ``
  - `closure` の引数: ``argument 0 of a closure of `g` is `n.1` (int), but the function takes tobj``
  - `apply` のオペランド: ``the callee of an apply is `n.1` (int), but an apply takes tobj``、``argument 0 of an apply is `n.1` (int), but an apply takes tobj``
  - `par` の子と結果: ``argument 0 of a par is `n.0` (int), but a par takes tobj`` (右の子は `argument 1`)、`` `t.2` (int) is bound to a par, which returns obj ``
  - `clause` のオペランド: ``the callee of a clause of `Counter.get` is `c.0` (int), but a clause takes tobj``、``argument 0 of a clause of `Counter.get` is `e.1` (int), but a clause takes obj``、``argument 1 of a clause of `Counter.put` is `n.1` (tobj), but the operation takes int``
  - `prim` のオペランド: ``argument 0 of `prim ev_new[State]` is `n.0` (int), but the primitive takes tobj``
  - `extern` の引数と結果: ``argument 0 of `Prelude.Num Int.+` is `s.0` (obj), but the extern takes int``、``argument 1 of `Prelude.Num Int.+` is (), but the extern takes int``、`` `t.1` (obj) is bound to `Prelude.Ord Int.<`, which returns enum ``。引数の数を確かめた後で、引数を左から、最後に束縛する変数を比べる
  - `apply` の結果: `` `t.0` (int) is bound to an apply, which returns tobj ``
  - `tail` の結果: ``a tail call to `k` returns int, but this function returns tobj``、``a tail apply returns tobj poly, but this function returns int``。印は ``a tail call to `h` returns lin, but this function returns unr`` の形である
  - `clause` と `prim` の結果: `` `t.0` (int) is bound to a clause of `State.get`, which returns tobj ``、`` `w.0` (int) is bound to `prim evv_get`, which returns tobj ``
  - `clause` の印の関数: `` `g$entry` is a clause of `Counter.put`, but its parameter 2 is tobj ``、`` `g$entry` is a clause of `Counter.get`, but it returns tobj ``
  - `tobj` のフィールド: R8 の文言を `tobj` に広げる。``field 0 of `Option` #1 is `x.3` (int), but the layout has tobj``、``argument 0 of a con of `Option` #1 is 5, but the layout has tobj``
- 境界の段の `box` と `unbox` の文言は次のとおりである。`int, float and enum` と `int, float or enum` の部分は、箱を要するスカラーの定義から作る
  - `box` のオペランド: `` `u.1` (unit) is boxed, but only int, float and enum values and Int and Float constants can be ``、``() is boxed, but only int, float and enum values and Int and Float constants can be`` (`#1`、`&g` も同じ形)
  - `box` の束縛: `` `b.2` (int) is bound to a box, which is tobj ``
  - `unbox` のオペランド: `` `p.0` (obj) is unboxed, but only tobj can be ``、``5 is unboxed, but only tobj can be``
  - `unbox` の束縛: `` `n.2` (tobj) is bound to an unbox, which gives int, float or enum ``
- 引き継ぐ検査は次のとおりである。2つの IR のすべての段で走る。直接呼び出しとクロージャの引数の数、文字列定数と関数の番号が表にあること、`extern` 命令の引数の数と、`extern Std.Par.par` を拒むこと (`` `Std.Par.par` is not an extern instruction ``。`Extern::Par` の行は std の宣言を型検査に結ぶためだけにあり、translate は呼び出しを `par` にする。[Core IR](core-ir.md) の「`par` の呼び出し」)、case の種類 (1つの `switch` の case が同じ種類であること、リテラルの case がフィールドを持たないこと)、`switch` が行き先を1つ以上持つこと、フィールドを束縛する case を持つ `switch` の scrutinee が RC の対象 (`obj` か `tobj`) の変数であること、リテラルの `switch` の `default`。Core IR のエフェクトの命令の検査は [Core IR](core-ir.md) の「verifier」にある
- 基本操作と `clause` の検査は次のとおりである。Core IR の EP の段と、LIR の5つの段のすべてで走る
  - `prim` の即値のエフェクトと操作がエフェクトの表にあり、引数の数が表の行と同じである。`evv_mask` の項目は、呼び出しの `mask` と同じく空でなく、番号の真に昇順に並び (同じエフェクトの項目が2つない)、どの項目も1つ以上の evidence を飛ばす (``the effects of `prim evv_mask[Counter, State]` are not in ascending order``、`` `prim evv_mask[]` masks no effect ``、``an item of `prim evv_mask[State*0]` skips no handler``)
  - `clause` の操作がエフェクトの表にあり、引数の数が「1 + 操作の引数の数」である
  - `ev_set_clause[E.op]` に渡す値が `&g` か、同じ関数の中の `closure g(..)` なら、g の `clause` の印が op に合い、同じ関数の中で evidence を定義した `ev_new[E]` の E にも合う
  - `clause` の印の関数が、`ev_set_clause` の引数のほかで値として参照されず、直接も呼ばれない。印のエフェクトと操作が表にあり、捕獲の後に evidence と操作の引数を受ける
- `par` の検査は次のとおりである。Core IR の2つの段と、LIR の5つの段のすべてで走る
  - `pair` は、`tobj` のフィールドをちょうど2つ持つ構築子である。VM は子の一様な結果を2つ並べて組を作るためである (``the pair of a par is `Box` #0, which does not have two tobj fields``)。表にない配置とタグの範囲は R8 と同じ文言で、主語を ``the pair of a par`` にする
  - LIR の `tail par` は、入口の段のほかの段で拒む (``a par is a tail call, but the join needs the parent's frame``)。縮約は `par` を `tail` にしないので、この形はパスの誤りである
- fork の関数の検査は次のとおりである。5つの段のすべてで、関数ごとの検査より前に走る
  - `par` を走らせうるプログラム (`Program::may_run_par`) は、fork の関数を持つエフェクト (操作を1つ以上持ち、`EffectInfo::never_only` が真のもの) の操作ごとに、その操作の `clause` の印を持つ `fork$E.op` と、`reperform$E.op` を持つ。VM が子の evidence をこの名前で作るためである ([Core IR](core-ir.md) の「`par` の子の中断」)。文言は `` `Fail.fail` has no fork entry, but the program may run par `` (`reperform$` は `no reperform entry`) と ``the fork entry is not a clause of `Fail.fail` `` で、`` in `fork$Fail.fail` `` のように探した関数の名前が付く
- 値の参照と版の検査は次のとおりである。範囲の段から後のすべての段で、本体の検査の後に走る
  - 値として参照する関数 (`&f` と `closure f(..)` の `f`) は、捕獲より後ろの引数と結果がすべて一様である。`clause` の印の関数は操作の ABI で呼ばれるので除く (`` `g` is used as a value with 0 captures, but its later parameters and its result are not all uniform ``)
  - `<f>$uniform` という名前の関数は、`<f>` があるときだけあり (``the variant has no function `f` ``)、`split` と `clause` の印を持たず、引数と結果がすべて一様である (``the variant does not have the uniform ABI``)
- 検査の順
  - それぞれの命令で、今ある形の検査を先に、R7 と R8 (境界の検査を含む) をその次に、範囲と所有の検査 (R5、R6) を最後に行う。`return` だけは、値の範囲と所有を確かめてから `ret` と比べる
  - 1つの呼び出しの中では、引数の数、引数の Repr (左から)、結果を束縛する変数の Repr の順に比べ、その後で範囲と所有を確かめる。`tail` では、引数の Repr の後で結果の Repr と印を呼び出し元の `rets` と比べる
- verifier は、着地の印の検査のほかでは生存解析を使わない。ブロックを番号の順にたどるだけで、反復しない。辺は前向きなので、ブロックに着いたときには入る辺がすべて出そろっている
  - 着地の印の検査は、v が呼び出しの直後で生きているかを、生存解析の入口の集合から決める。集合は印のある関数でだけ、関数ごとに1回求める。呼び出しのたびに、そのブロックの終端と後ろの文から逆にたどる
  - 支配木は Cooper-Harvey-Kennedy の方法で求める。番号の順が辺の向きに沿っているので、1回たどれば決まる。2つのブロックの共通の支配者は、支配木の skew-binary の飛び先 (Myers) を使って、深さの対数の歩みで求める。1段ずつ登ると、深さの違う辺が多く1つのブロックに合流したとき (`a || b || ...` の真の行き先など)、時間はブロックの数と深さの積になる
  - 変数ごとに定義の位置 (ブロックと文の番号。引数は -1) を記録し、支配は支配木の前順と後順の番号で調べる
  - 所有の多重集合は、合流を待つブロックごとに疎に持つ。辺ごとに所有の多重集合を写して比べるので、その時間は辺の数と所有の大きさの積に比例する

### Kind の印の検査

印は Core IR で決まり、後のパスは変えない (box の挿入は `unr` のスカラーを足し、ほかの変数には元の印を写すだけである。開くパスは葉に、形の葉の印と変数の印の小さいほうを付ける)。そのため、誤った印を付けたパスの直後の段で見つかる。

- V2: 値の受け渡しで、送り手の印を受け手の印に渡せる (`Mark::passes_to`)。つまり `lin` の値を `unr` の受け手に渡さない。`unr` の値を `lin` の受け手に渡すのは許す。各段で Repr を比べる所と同じ所で走らせる。`lin` の値を持つ値に `unr` の印を付ける誤りは、作る所の検査が止める
  - Core IR の2つの段と LIR の入口の段: `jump` の実引数と行き先の引数、`return` の値と関数の `ret_mark`
  - LIR の境界の段: 上に加えて、直接の呼び出しの引数と仮引数、結果と戻り値の印、`closure` の引数と、クロージャにする関数の捕まえる引数、`tail` の結果と呼び出し元の `ret_mark`、`apply` の位置 (いつも `poly` なので、実際には何も拒まない)、`par` の子と結果 (`apply` と同じく `poly` の位置で、何も拒まない)、`extern`、`prim`、`clause` の引数と結果とそれぞれの表の列 (extern の行、基本操作の表、操作の表)、`clause` の印の付いた関数の引数と戻り値と操作の表の列
  - `perform`、`handle`、`resume` (EP の前の Core IR にだけある) の受け渡しは比べない。EP がそれらを `clause` と基本操作に置き換えた後の段で比べる
  - `clause` の呼ばれる側 (`ev_clause` が返す入口のクロージャ) は `unr` でなければならない。`clause` の呼び出しの引数 0 の evidence と、`clause` の印の付いた関数の evidence の引数も、`unr` として比べる
  - `never` の操作の `clause` の結果を束縛する変数は、Repr と同じく、どの使いでも V2 と作る所の検査の対象にしない。使いには制御が届かないためである (上の「`never` の操作」)。Core IR の変換の段では、`never` の操作の `perform` の結果を束縛する変数も同じ理由で対象にしない
  - 定数はどの印とも合う
  - `par` の子は `poly` の受け手なので、`Lin` の値を捕まえた `lin` の閉包も渡せる。`par` は子を消費するので、Perceus はほかの呼び出しと同じく、後でまた使う子を `dup` する。`lin` の子は1回だけ使うので、RC の命令が付かない。`lin` の子を `par` の後でまた使うと、`dup` を入れれば V1 が、入れなければ所有の検査が拒む。結果は `poly` なので、`unr`、`poly`、`lin` のどの印の変数にも束ねられる
- V1: `unr` でない変数に `dup` と `decref` を置かない。`dup` と `decref` は Perceus の後にしかないので、所有の段だけで走らせる。`unr` でない値を手放す命令は `drop` と `release` である
- 作る所の検査: `con` と `closure` が束縛する変数の印は、引数の印の結び以上でなければならない (順序は `unr` < `poly` < `lin`)。V1 だけでは、誤って `unr` を付けた `List FileIn` や `FileIn` を捕まえたクロージャが `dup` されても気づけないので、作る所で拒む。Core IR の2つの段と LIR のすべての段で走らせる。定数は `unr` として結び、`never` の束縛は数えない
- 誤りの文言は次のとおりである
  - V1: `` `x.1` (lin) is duplicated, but only unr can be ``、`` `x.1` (poly) is released, but only unr can be ``
  - `jump` と `return`: ``a jump to b2 passes `x.1` (lin) to `p.3` (unr) ``、`` `x.1` (lin) is returned from a function that returns unr ``
  - 直接の呼び出しの引数: ``argument 0 of `g` is `x.1` (lin), but the function takes unr``。`closure` の引数も同じ形である (``argument 0 of a closure of `g` ``)
  - `extern`、`prim`、`clause` の引数: ``argument 0 of `Std.FileIn.open` is `x.1` (lin), but the extern takes unr``、``argument 1 of a clause of `Counter.put` is `x.1` (lin), but the operation takes unr``、``the callee of a clause of `Counter.get` is `c.0` (lin), but a clause takes unr``
  - 結果: `` `t.0` (unr) is bound to `Std.FileIn.open`, which returns lin ``。`prim`、`clause`、呼び出しも同じ形である
  - `clause` の印の関数: ``parameter 2 of `g$entry`, a clause of `Counter.put`, is `x.2` (unr), but the operation takes lin``、`` `g$entry`, a clause of `Counter.get`, returns lin, but the operation returns unr ``
  - 作る所: `` `p.2` (unr) is bound to a con of `(,)` #0, whose arguments join to lin ``、`` `c.1` (unr) is bound to a closure of `k`, whose captures join to poly ``

## 実行の意味

LIR を実行する [VM](vm.md) と後のネイティブのコードは、次の意味に従う。

- 実行は入口の関数から始める。始めは、空の `evv` と yield なしである (上の「実行の文脈」)
- `jump` は実引数をすべて読んでから、行き先の引数に書き、行き先の先頭へ進む。同じ関数の中で制御を移すだけで、フレームを積まない
- 末尾でない呼び出しは、戻ったらその文の `let` の変数に結果を入れ、次の文から続ける。末尾呼び出し (`tail`) は、呼び出し元のフレームを積まない (上の「末尾の位置の呼び出し」)
- 変数の消費は所有権の移動 (move) とし、値を複製するのは `dup` 命令だけにする。`switch` の scrutinee、`unpack` の値、`unbox` のオペランド、基本操作の借用の引数 (上の「基本操作」) は読むだけで、所有権を動かさない。Perceus の所有権の規則と実行の動作を1対1に対応させるためである
- `switch` は scrutinee を読むだけで、参照の数を変えない。case を選び、フィールドの値をフィールドの変数に書いてから、行き先の先頭へ進む。ヒープからは何も取り出さず、複製もしない。行き先は `switch` の前の所有をそのまま持って始まり、scrutinee を手放す `decref` か `release` と、フィールドの `dup` は、Perceus が所有の要る所に置く (上の「Perceus」)。scrutinee が即値のタグならその case に入る。物体なら、そのタグの case に入り、合う case がなければ `default` に進む。`Int` と `String` は case の値と比べる。`String` の scrutinee も `switch` では手放さず、行き先が手放す
- `unpack` は、1つの case の `switch` と同じく値を読むだけで、フィールドの値をフィールドの変数に書く
- 構造体の `unbox` も箱を読むだけで、名前を書いたフィールドの値を葉の変数に書く。箱がダミーなら、葉にダミーの値を書く (上の「ダミーの値」)
- 複数の値を返す呼び出しは、戻ったら結果を値の順に `let` の変数に入れる。`return (a1, .., an)` は値を並べて返す
- 末尾呼び出しの結果は、Repr を変えずに呼び出し元の結果になる (上の「末尾の位置の呼び出し」)。`apply` は引数を一様な語のまま渡し、一様な語を受ける
- `release x L #t(..)` は、x の参照を1つ手放し、名前を書いた位置のフィールドの参照を残す。x が一意なら箱を空け、残さないフィールドを手放す。そうでなければ、残すフィールドを `dup` してから x を手放す ([オブジェクトモデル](object-model.md) の「核の操作」)
- token の命令は、次の `eml_rt` の部品の列になる ([オブジェクトモデル](object-model.md) の「reuse」)。VM とネイティブのコードは同じ列を出す
  - `let r: token = release x L #t(..)` は、x が一意なら (`is_unique`)、残さないフィールドを `decref` してから、x の箱を token にして r に書く (`into_token`)。そうでなければ `release` と同じ列 (`is_last` で分ける) で x を手放し、空の token を r に書く
  - `con L #k(..) reuse r` は、r が空なら `con` と同じく確保して (`alloc_data`)、書かない印の引数も含めてすべてのフィールドを書く。空でなければ、`in place` なら `reuse_in_place` で、そうでなければ `con_at` で `meta` を書いて箱を得てから、書かない印のない引数だけを書く
  - `drop_token r` は、token の箱を空ける (`drop_token`)。空の token なら何もしない
- `prim` は、上の「基本操作」の表のとおりに文脈と evidence を読み書きする。借用の引数は呼ぶ側が持ったままなので手放さず、ほかの引数は所有した参照として受け取り、結果に残さないものを手放す。結果は所有した参照である。`evv_mask` が残すリストの残りと、`ev_renew` が新しい evidence に入れる入口の表は、基本操作が自分で `dup` する
- `clause` は `apply` と同じく関数の値を呼ぶ。操作の ABI は値の表し方を変えないためである (上の「節の呼び出し」)
- ダミーの値 (`prim dummy()`、`0`、`0.0`、`#0`、`()`) は、上の「ダミーの値」の約束を守る。`dup`、`decref`、`drop` は何も起こさず、`box` と `unbox` は誤りにしない
- `drop k` は、継続のクロージャが捕まえていた値を、解放の連鎖で1回ずつ解放する。`Lin` の値の破棄処理は物体の解放そのものなので、捕まっていた `FileIn` は、この解放で読み出し口が捨てられて閉じる ([オブジェクトモデル](object-model.md) の「読み手」)
- 中断 (`never` の操作、`try_io` が捕まえる IO の失敗、`try_panic` が捕まえる `Panic`、`exit`) は継続を組み立てず、タスクのスタックを巻き戻す。外すフレームが、中断の通った命令 (下の「巻き戻しの地点」) の直後に所有している RC の対象の値を、所有の数だけ手放し、生きている token を `drop_token` で手放す。`FileIn` はこの解放で閉じる。着地の印の evidence が中断の宛先と同じ `apply` (上の「着地の印」) に着いたら、その結果にダミーを入れ、次の文から続ける。`exit` の中断の宛先はどの印とも合わないので、実行の底まで巻き戻す。手順は [VM](vm.md) の「巻き戻し」にある
- 下の「実行時エラー」は巻き戻さず、評価全体を止める。分担は [VM](vm.md) の「中断と実行時エラーの分担」にある
- `extern` 命令は、引数を受け取ってその場で実行し、値を返す。継続の最下部には、プログラムの終わりを表す枠があるだけで、`IO` の handler はない。`println` は Host に書く。`open` は `FileIn` の物体を確保し、`FileIn` の読み (`read_all`、`read_line_raw`、`read_all_bytes`、`read_line_bytes_raw`、`read_chunk`) は `FileIn` を借りて読み、読んだ文字列かバイト列を返す。`read_chunk` は、受け取ったバイト列に足して返す。`close` は `FileIn` を解放する ([エフェクトと handler](effects.md) の「組み込みの `IO`」)。標準入力のハンドルも、`open` が物体を確保し、同じ名前の5つの読みがハンドルを借りて読み、`close` が解放する ([標準ライブラリ](stdlib.md) の「`Stdin`」)。標準出力と標準エラーのハンドルの書き込みは、データを借りて Host に書き、受け取ったハンドルを返す ([標準ライブラリ](stdlib.md) の「`Stdout` と `Stderr`」)
- 実行が正しく終わったとき、`evv` は空で、yield の状態はなしでなければならない (上の「`evv` の不変条件」)。そうでなければ内部の誤りである。残った evidence がリークに見えないように、この検査はモードによらず、検査モードのリークの検査の前に行う
- 呼び出しの深さの扱いは [VM](vm.md) の「スロットとスタック」にある
- 名前付きのレコードとタプルは、コンストラクタが1つの `data` と同じ物体で、位置で分解する ([直積型とレコード](records.md))

### 巻き戻しの地点

中断が通りうる命令だけを巻き戻しの地点にする。中断しない関数のフレームを中断が通ることはないので、その関数の呼び出しには巻き戻しの表の行が要らない。

- `eml_lir::unwind::may_abort(program) -> Vec<bool>` は、関数ごとに中断しうるかを `FnIdx` の番号で返す。関数は、次のどれかを文か終端に持つとき中断しうる
  - 中断しうる関数の直接の呼び出し。`tail call` も含む
  - `apply` と `clause` の呼び出し。末尾のものも含み、呼ぶ相手によらない。すぐに再開する節は、操作を起こしたフレームの上でその場で動く (上の「節の呼び出し」)。そのため row が `<State>` のような閉包の呼び出しにも、節の本体の `panic` や IO の失敗が通る
  - `Pure` でない extern
  - `prim yield_abort` と `prim child_abort`
  - `par`。子の中断を、join の後に親で起こし直すためである。`par` は末尾の位置の呼び出しにならないので、いつも末尾でない
- 「中断しうる」は最小の不動点で求める。中断する文を持たない相互再帰は中断しない。求め方は、上の直接の呼び出し以外の文を持つ関数から始めて、直接の呼び出しの辺を呼ぶ側へたどり、印を広げる。時間は関数と命令の数に比例する
- 巻き戻しの地点は、上の文のうち末尾でないものである。つまり、中断しうる関数の末尾でない直接の呼び出し、末尾でない `apply` と `clause`、`par`、`Pure` でない extern、`prim yield_abort` と `prim child_abort` である。中断は地点の命令の中か、その呼ばれる側で始まる。中断しない関数の中では始まらないので、中断が通るフレームはいつも地点の命令の直後で止まっている
- `points` は、`may_abort` をプログラムに1回だけ求め、所有の段の走査に渡す (上の「verifier」、[VM](vm.md) の「巻き戻しの表」)
- 名前が同じ関数の地点は、REPL のどの入力で作っても同じになる。入力の LIR が持つのはその入力の入口から届く instance だけだが、関数が中断しうるかは、その関数から直接の呼び出しでたどれる関数だけで決まる。たどれる関数はいつも同じプログラムにあり、名前が同じ関数は同じコードなので ([VM](vm.md) の「追記」)、たどれる関数とその印も同じになる

## 実行時エラー

次の場合は実行を止めて実行時エラーにする。CLI の終了コードは 1 で、割り込みで止まったときだけ 130 である ([VM](vm.md) の「上限と割り込み」)。

- `Int` のオーバーフローとゼロ除算 ([宣言](declarations.md) の標準の演算子の表)
- `Std.Float` の `truncate`、`floor`、`ceil`、`round` の結果が `Int` に収まらないとき (`` float `<x>` does not fit in `Int` ``。`x` は `show` の形)、`Float.to_fixed` の桁数が 0 から 100 の外のとき (`` `to_fixed` takes 0 to 100 digits, not <n> ``) ([標準ライブラリ](stdlib.md) の「`Std.Float`」)
- `Std.Char` の `of_int` の引数がスカラー値でないとき (``int `<n>` is not a Unicode scalar value``) ([標準ライブラリ](stdlib.md) の「`Std.Char`」)
- `Std.Byte` の `of_int` の引数が 0 から 255 の外のとき (``int `<n>` does not fit in `Byte` ``) ([標準ライブラリ](stdlib.md) の「`Std.Byte`」)
- `FileIn.open`、`FileOut.create`、`FileOut.append` が開けないとき (``cannot open `<path>`: <reason>``)、`FileIn.read_all` か `FileIn.read_line` が読めないとき (``cannot read `<path>`: <reason>``)、`FileOut` の書き込み、`flush`、`close` が書けないとき (``cannot write to `<path>`: <reason>``)、`Proc` の起動 (`run`、`output`、`capture`、`spawn` など) が子を起動できないとき (``cannot run `<program>`: <reason>``)、`Child` の読みが子の標準出力を読めないとき (``cannot read the output of `<program>`: <reason>``)、`Child` の書き込みが子の標準入力に書けないとき (``cannot write to `<program>`: <reason>``)、`Proc.wait` か、待つ起動の関数 (`run`、`output`、`capture` など) が子を待てないとき (``cannot wait for `<program>`: <reason>``)、標準入力を読めないとき (``cannot read the standard input: <reason>``)。`reason` は `not found`、`permission denied`、`not available`、`I/O error` のどれかである
- 読んだ内容が UTF-8 でないとき。ファイル (``` `<path>` is not valid UTF-8 ```)、標準入力 (`the standard input is not valid UTF-8`)、子の標準出力 (``the output of `<program>` is not valid UTF-8``)、引数 (`argument <index> is not valid UTF-8`。`Env.args ()` のリストの 0 から数えた位置)、環境変数 (``environment variable `<name>` is not valid UTF-8``) である
- `Proc.capture` と `capture_bytes` の子が 0 でない終了コードで終わったとき (``command `<program>` exited with code <code>``。シグナルなら 128 + 番号) ([標準ライブラリ](stdlib.md) の「`Proc`」)
- 文字列の位置が範囲の外のとき (`string index <index> is out of range for length <len>`)、`String.slice` の範囲が文字列の外のとき (`string slice <start>..<end> is out of range for length <len>`) と、端が文字の境界にないとき (`string slice <start>..<end> is not on a character boundary`)、`String.char_at` の位置が文字の境界にないとき (`string index <index> is not on a character boundary`)、`Env.arg` の位置が範囲の外のとき (`argument index <index> is out of range for <count> arguments`)。最後のものは標準ライブラリの中だけで起きる ([標準ライブラリ](stdlib.md) の「実行時エラー」)
- バイト列の範囲が外のとき (`bytes slice <start>..<end> is out of range for length <len>`。`Bytes.slice`、`append_slice`、`truncate` で、`truncate n` の範囲は `0..n`)、バイト列の位置が外のとき (`bytes index <index> is out of range for length <len>`。`Bytes.get`、`set` の位置と、`Bytes.find_from` と `find_byte_from` の開始位置)、`Bytes.make` の長さが負のとき (`bytes length <len> is negative`) ([標準ライブラリ](stdlib.md) の「`Bytes`」)
- `String.decode` に渡したバイト列が UTF-8 として妥当でないとき (`invalid UTF-8 at byte <at>`。`at` は最初の妥当でないバイトの位置) ([標準ライブラリ](stdlib.md) の「`String`」の「`Bytes` との変換」)
- 配列の添字が範囲の外のとき (`array index <index> is out of range for length <len>`) と、`make` の長さが負のとき (`array length <len> is negative`)
- 標準のストリームのハンドルが生きている間に、同じストリームをもう一度 `open` したとき (`the standard input is already open`、`the standard output is already open`、`the standard error is already open`) ([標準ライブラリ](stdlib.md) の「ハンドル」)
- `println` と `eprintln` が書けないとき (``cannot write the output: <e>``)。`e` は書き込みの誤りの `io::Error` の文言である
- 実行を止めたとき。割り込み (`interrupted`)、確保量の上限 (`the memory limit is exceeded`)、深さの上限 (`the call depth limit is exceeded`) である ([オブジェクトモデル](object-model.md) の「実行の制御」)
- 検査モード (`eml run --debug-heap`) のヒープの誤り。解放済みの物体の使用 (`use of a freed object`)、`lin` ビットの立った物体の `dup` (`a Lin object is shared`)、終わりに残った物体 (リーク) などである ([オブジェクトモデル](object-model.md) の「報告する誤り」)。どれもコンパイラの誤りでしか起きない。速いモードはこれらを確かめない ([オブジェクトモデル](object-model.md) の「受け入れる制約」)
- 内部の誤り (`internal error: …`)。verifier を通った IR では起きないはずの形を、VM の見張りが見つけたときである ([VM](vm.md) の「誤り、止めること、終わり」)
- 利用者の `panic s` を捕まえる `try_panic` がないとき (`panicked: <s>`)

`try_io` の中で起きた I/O の失敗の一部は、実行時エラーにせず `try_io` に届ける。オーバーフロー、ゼロ除算、`Float` の変換と `to_fixed` の誤り、`Char.of_int` の誤り、妥当でない UTF-8 のバイト列、範囲の外の位置、標準のストリームの2つ目の `open` は `Panic` で、`try_panic` の中で起きたら実行時エラーにせず `try_panic` に届ける ([VM](vm.md) の「`IO` と `Panic` への中断」)。上の一覧の文言は、捕まえる `try_io` か `try_panic` がないときの表示で、`try_panic` が返す `Err` の文言でもある。

extern の実装は、VM と後のネイティブのコードが使う `eml_rt` にある ([オブジェクトモデル](object-model.md) の「extern」)。誤りの文言は `eml_rt` のもので、上の一覧と1文字も違わない。

`eml_rt` の Host が出力をまとめて書くときは、書き込みの誤りが見つかるのは、後でバッファがあふれた `println` か、プログラムの終わりの flush である。そのため、`println` の誤りに付く位置は、書けなかった行を書いた `println` でなく、誤りを見つけた `println` の位置になる。終わりの flush の誤りは位置を持たない。

実行時エラーは、誤りの種類 (`fault`)、そのとき実行していた LIR の関数の名前 (`function`)、省略できる位置 (`at`) を持つ。位置を持つ `extern` 命令の実行が起こした誤り (その中のヒープの誤りを含む) にだけ、その命令の位置を付ける。位置は Core IR から写したもので ([Core IR](core-ir.md) の「位置」)、ファイルは `Program.files` の添字である。引数の読み出し、`dup`、`decref`、`release`、`switch`、`unpack`、`box`、`unbox` の誤りと、位置を持たない `extern` 命令の誤りには付けない。安全点で止めたときも位置を付けない。`FileIn` の読み、標準入力の読み、子の起動と `Child` の読み書きと `Proc.wait` の中で止めたときは、extern の誤りなので位置を付ける。標準ライブラリの関数の中の extern なら、位置は `<std>/…` を指す。

位置があるときは、次のように表示する。`path` は `Program.files` の表示用のパスである。

```
{fault}
  at {path}:{line}:{column}
```

位置がないとき (extern 命令の外で起きた誤り、位置のない手書きの IR) は ``{fault} in `{function}` `` と表示する。リークの表示は位置を持たない。リークと回復の失敗の表示の形は [VM](vm.md) の「誤り、止めること、終わり」にある。
