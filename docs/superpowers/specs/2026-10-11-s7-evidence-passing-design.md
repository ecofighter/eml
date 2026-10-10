# S7 evidence passing

位置づけ: 作業の設計。ロードマップの S7 を定める。S7 を終えたら、決まったことを `docs/spec/` と `docs/implementation/` に移し、この文書を削除する。

## 目的

エフェクトの操作と handler を、Core IR から Core IR への evidence passing (EP) の変換で実装する。変換の後の IR は、普通の呼び出しと分岐、節の呼び出し、決まった表の基本操作だけになる。S8 の LIR、S10 の VM、後のネイティブ化は、`handle`、`perform`、`resume`、`mask` を持たない入力を受け取る。

完了の条件は、ロードマップのとおり次の2つである。

- 変換の有無で、全 UI テストの出力が一致する
- 新しい指標で、すぐに再開する操作の `perform` の費用が、`perform` と handler の間の呼び出しの深さによらないことを確かめる。再開する操作の yield は、間のフレームの数に比例する。これは方式の性質なので、同じテストに書いて確かめる

## ロードマップと設計の文書から改めること

ロードマップの S7 の節と [evidence passing の設計](../../future/evidence-passing.md) の決定を、次のとおり改める。

- **evidence を実行時にラベルで引く**: evidence を、内側の handler を先頭にした永続リストに並べ、`perform` はエフェクトの番号で先頭から探す。閉じた row の静的な添字と open (閉じた row の関数を広い row の文脈から呼ぶときにベクタを合わせる包み) は作らない。そのため、Core IR に row そのものとラベルの順序は持たせない。Core IR が持つ row の情報は、関数と `apply` の「yield しうるか」の印だけである (下の「単相化と `yields` の印」)。理由は「採らなかった案」に書く
- **row を yield しうるかで単相化する**: instance の鍵に、row 変数ごとに「yield しうる row が入ったか」のビットを足す。handler を使わないプログラムには、EP が何も足さない。`docs/spec/core-ir.md` の「row は単相化しない」を改める
- **操作の Repr が効く範囲**: 操作の宣言した Repr は、`perform` の引数と結果、つまり in-place の節の入口の引数と結果に使う。継続を再開する値と、yield で運ぶ節の引数は一様のままにする。EP の後は継続の再開も yield も一様な `apply` で動くため、そこに操作の Repr を通すには、継続ごとと節ごとに別の呼び出しの規約が要るからである
- **出力の命令の種類**: ロードマップは、EP の出力を「既存の命令の種類と基本操作だけ」にし、box の挿入と Perceus に特別な規則を足さないとしていた。操作の Repr を `perform` の境界に通すために、操作の ABI を知っている呼び出し `Call::Clause` を1つ足し、box の挿入、T3、縮約、verifier にその規則を足す。Perceus には規則を足さない。S8 では、`Call::Clause` は LIR の、操作の ABI を持つ間接呼び出しにそのまま写す
- **すぐに再開する節の範囲**: その場で実行するのは、すぐに再開し、かつ本体に yield の地点がない節に限る。設計の文書の under (節の中の操作を handler の外へ届ける `evv` の切り替え) は作らない。本体が外のエフェクトを呼ぶ節は、再開する節として扱う (下の「`perform` と節の3つの形」)
- **evidence は実行ごとに作る**: handle の実行と、継続の再開のたびに、新しい evidence を作る。状態は evidence の欄に置く。handler の実行中にその handler 自身の継続を再開しても (下の「再開と状態」の例)、2つの実行が欄を共有しない

ロードマップの論点は、次のとおり決める。

| 論点 | 決定 |
|---|---|
| 「yield 中か」の確認の費用と、合流のブロックとの組み合わせ | 確認は呼び出しごとに文脈の旗を1回読む。継続の関数を、yield の地点と合流のブロックを根とする区間ごとに切り出す (下の「継続の関数」) |
| 開いた row で探すときのキャッシュ | 作らない。`evv` は書き換えないので、後で場所ごとのキャッシュを足せる。「処理系」の項目に移す |
| `never` の中断で `Lin` の値をどう drop するか | 継続を普通に組み立ててから、`prompt$` が `drop` する。途中のフレームは、生きている `Lin` の値に `decref` を置かない |
| EP が作る継続のクロージャの Kind の印 | S8 の決定 (`once` の継続は `lin`、`multi` は `unr`、多相な関数のフレームの続きは `poly`) に、下の「S8 への申し送り」を足す |
| 操作の宣言した Repr | 上のとおり、`perform` と in-place の入口の境界に使う |

## 実行時のモデル

### 実行の文脈

インタプリタの機械は、次の2つを持つ。後の VM とネイティブでは、これを実行の単位ごとに持つ。S13 の協調的な `par` でタスクを切り替えるなら、タスクごとに持つか、切り替えで退避する。

- `evv`: 今有効な evidence の並び。内側の handler を先頭にした永続リストで、書き換えない。handler を設けるたびに、先頭に足した新しいリストを作る
- yield の状態: なし、yield 中、中断中のどれか。yield 中と中断中は、宛先の evidence、節の本体のクロージャ、組み立て中の継続 κ を持つ

`evv` を関数の引数でなく文脈に持つので、yield しうる関数とそうでない関数の呼び出しの規約は同じである。そのため、下の `yields` の印が保守的でも、規約は食い違わない。

### evidence

evidence は、handler の1回の実行を表すヒープの物体で、次を持つ。

- 入口の表: エフェクトの番号と、操作ごとの in-place の入口のクロージャ (下の「`perform` と節の3つの形」) を持つ物体。handle で入れた後は変えない
- 状態の欄 (`tobj`)
- 外側の `evv`: その実行の外側で有効な `evv`。作るときに決め、変えない

handle の実行と、継続の再開のたびに、新しい evidence を作る。入口の表は handle のときに1つ作り、再開で作る evidence はその表を共有し、状態と外側の `evv` だけを新しくするので、作る費用は操作の数によらない。yield の宛先は evidence の物体そのもので比べるので、目印の番号は持たない。

状態の欄だけは書き換える。書き換えるのは、その evidence の実行が動いている間の、in-place の節 (`ev_take_state`、`ev_set_state`) と `prompt$` (状態の取り出し) だけである。別の実行は別の evidence を持つので、欄を共有しない。外側の `evv` は作るときの `evv` で、その evidence 自身を含まない。

状態の欄を通る参照の輪は、一時的にはできうる。外側の handler O の本体の中で内側の handler I を動かし、I の継続 `k_I` をデータに包んで O の状態に入れると、`evO.状態 → k_I → evI → evI.外側の evv → evO` の輪になる。`prompt$` は、どの出口 (下の「handle と `prompt$`」の 0〜3) でも状態の欄を空にするので、O の実行が終われば輪は切れる。これを不変条件とし、`debug_heap` で確かめる UI テストを足す。

### 探す

- `ev_lookup[E]` は、`evv` を先頭からたどり、エフェクトの番号が E の最初の evidence を返す。費用は、今有効な handler の数に比例し、呼び出しの深さにはよらない
- `evv_mask[effects](w)` は、`effects` の多重集合の各エフェクトについて、w の先頭から合う evidence をその数だけ除いたリストを作る。除く位置までの節点を写し、残りは共有する。evidence の物体は写さない

## 単相化と `yields` の印

### row の具体化の記録

- 型検査は、参照ごとの具体化 (`Instantiation`) に、row 変数の具体化 `rows: Vec<RowYield>` を足す。並びは、参照先のシグネチャの row 変数の順 (`generics.row_vars` の順) である。今の `Instantiation` は型変数の型引数だけを持つ
- `RowYield` は、具体化した row が yield しうるかを、参照を含む定義の row 変数に対して表す
  - `Always`: handle できるラベル (extern のエフェクトでないラベル) を含む。または、末尾が参照を含む定義の row 変数でない rigid な row 変数である (操作のシグネチャの row 変数を、節の中で具体化したものなど)
  - `Never`: handle できるラベルを含まず、閉じているか、末尾が型検査で最後まで決まらなかった row 変数である。決まらなかった row 変数には、どのエフェクトも求められていないので、yield しうる値はそこを通らない (`fn h -> h 1 + 1` を使わずに置いた形)
  - `Var(i)`: handle できるラベルを含まず、末尾が参照を含む定義の i 番目の row 変数である。ラムダ、handle の本体、節の中の参照では、それらを含むトップレベルの定義 (instance のメソッドの関数と既定のメソッドを含む) の row 変数を指す
- `eml_types` は、関数の型、ラムダの型、`apply` の矢印の row を同じ `RowYield` に分類した結果も、側の表に記録する。分類は、rigid な row 変数がどの定義のものかを型検査が知っている間に行う。書き出した型の rigid な末尾は名前でしか区別できず、操作の row 変数と定義の row 変数を取り違えるためである。translate はこの表を読む
- 型検査のテスト (`eml_types` の `instantiations.rs`) に、`rows` の記録を足す

### row のビットと instance の鍵

- instance の鍵を (関数, 型引数, row のビット) にする。row のビットは、参照の `rows` の各要素を、参照を含む instance のビットで解いたものである (`Always` は立つ、`Never` は立たない、`Var(i)` は含む instance の i 番目のビット)
- メソッドの参照の row 変数は、メソッドのシグネチャの row 変数である。instance のメソッドの関数と既定のメソッドの関数は、メソッドのシグネチャをそのまま row 変数に持つので、ビットを同じ順に写す。導出とタプルの生成器が作る関数は row 変数を持たない
- 入口の instance のビットは、すべて立てる。安全側の選び方で、`main` は row 変数を持たないので影響しない
- ビットの組は row 変数が k 個なら 2^k 通りなので、instance を集める手順は止まる。一様な位置の議論 (`docs/spec/core-ir.md` の「一様な位置と制約付きの多相再帰」) は型変数だけのもので、row には当てはめない
- 名前: 立ったビットの row 変数だけを、`@[…]` の後ろに `<e>` の形で書く。`map@[Int, String]<e>`、型引数のない関数なら `twice<e>`、長い名前の番号の形なら `ident@3<e>` である。2つ立てば `<e, f>` とシグネチャの順に書く。ビットがすべて立たない instance の名前は今と変わらない。`<` と `>` は今のテキストの形の名前の文字でないので、こういう名前は空白を含む名前と同じく引用符で囲む

### `yields` の印

- `CoreFn` に `yields: bool` を足す。テキストの形では、`internal` の後に `yields fn f(..)` と書く (`internal yields fn main$lambda0(..)`)
- translate は、関数の型の「引数の数番目の矢印」の row が yield しうるとき (`RowYield` をその instance のビットで解いて立つとき) に立てる。本体はその矢印の row で動くためである ([型と Kind](../../spec/types.md) の「推論」)。`f : Int -> <State> (Int -> Int)` の `f x = …` では1番目の矢印である。引数のない値は純粋なので立てない
- トップレベルの関数は宣言の型から、ラムダは外側の instance の代入をかけた自分の型から決める
- handle の本体はいつも立てる。本体の row は扱うエフェクトを含むためである
- 操作の節と `return` の節は、handle を含む関数の印と同じにする。節は handle の外側の row で動き、その row は handle を含む関数が動いている row である
- `op$` は、その操作を呼ぶ形なので立てる。`cont$` と `cont$state` は、どの文脈からも呼ばれる補助の関数なので立てる。`con$`、`$externN`、導出とタプルの生成器が作る関数、`tag$` は立てない
- box の挿入が足す `f$boxed` は、`f` の印を写す。S8 で LIR に写すためである
- EP が作る関数の印は、下の各項目で決める

### `apply` の印と分け方

- translate は今、`mask` の変わる境目で、余った引数を渡す `apply` を分けている。これに加えて、yield しうる矢印の後でも `apply` を分ける。実行時の `apply` は、呼ばれる関数の引数の数で引数を分けて順に呼ぶ (eval/apply) ので、1つの `apply` の途中の矢印で yield すると、残りの引数を渡す処理が継続に入らないためである。分けた後の1つの `apply` の中では、最後の矢印のほかは yield しない
- `Call::Apply` に `yields: bool` を足し、translate が、その `apply` が受け持つ矢印のどれかが yield しうるなら立てる。分け方から、これは最後の矢印が yield しうるときである
- テキストの形では `yields apply c.0(x.1)` と書く。`mask` があれば `mask [State] yields apply …` の順にする
- `Call::Resume` から EP が作る `apply` の印は、その命令を含む関数の印と同じにする。`k` の最後の矢印の row は handle の外側の row である。生の継続をラムダや handle の本体が捕まえて再開する形でも、それを含む関数の row は `k` の row を含まなければならないので、この印は安全側である

## Core IR に足すもの

### 操作の表の Repr

- `OperationInfo` に、引数の Repr の列 `params` と、結果 `ret: OpRet` を足す。操作のスキームの型から決め、型変数は `tobj` にする。データの配置のフィールドの Repr と同じ考え方である。`effect Counter where put : Int -> Unit` の `put` は `int` を受けて `unit` を返し、`effect State s` の `get` は `tobj` を返す
- `OpRet` は `Repr` か `Never` である。`never` の操作の結果は `Never` とし、呼び出した側の位置と比べない (今の `never` の `perform` の規則を移す)
- 今の `arity` は `params` の長さになるので消す。`resumable` は残す
- テキストの形の `effect` の宣言は、今の `get/1` を、関数の引数と `ret` に合わせた綴りに替える (`effect Counter { get(unit) -> int, put(int) -> unit }`、`effect Fail { never fail(tobj) -> never }`)

### 基本操作

- 新しい右辺 `Rhs::Prim(Prim, Vec<Atom>)` を足す。`Prim` は基本操作の種類と、その即値 (エフェクトの番号、操作の番号、エフェクトの多重集合) を持つ。テキストの形は `prim ev_lookup[State]()`、`prim evv_mask[State, State](w.3)` のように、即値を `[…]` に書く
- 引数と結果の Repr は、次の表で決まる。引数と結果は、直接の呼び出しの引数と結果と同じく互換の位置で、合わなければ box の挿入が変換を入れる。状態とクロージャの引数には、translate が決めた Repr の値 (`State Int` の状態なら `int`、状態のない handler なら `()`) が来るためである
- 引数はすべて消費し、結果は所有した参照である (extern の行と同じ)
- 表は `eml_core_ir` に置き、`eml_extern` には入れない。`eml_extern` の表は `std/` と1対1に結び付けるテストを持ち、基本操作はソースに現れないためである

| 基本操作 | 引数 | 結果 | 動作 |
|---|---|---|---|
| `evv_get` | なし | `tobj` | 今の `evv` |
| `evv_set` | `tobj` | `unit` | `evv` を替える。元の `evv` は手放す |
| `evv_push` | `tobj`、`obj` | `tobj` | リストの先頭に evidence を足す |
| `evv_mask[effects]` | `tobj` | `tobj` | 上の「探す」 |
| `ev_new[E]` | `tobj` (状態)、`tobj` (外側の `evv`) | `obj` | 新しい入口の表 (欄は空) と、それを指す新しい evidence |
| `ev_set_clause[op]` | `obj`、`tobj` | `unit` | 入口の表の欄に入れる。handle の場所だけが、本体を始める前に使う |
| `ev_renew` | `obj`、`tobj` (状態) | `obj` | 元の evidence と同じ入口の表を指し、状態を引数に、外側の `evv` を今の `evv` にした新しい evidence |
| `ev_lookup[E]` | なし | `obj` | 上の「探す」 |
| `ev_clause[op]` | `obj` | `tobj` | 入口の欄を読む |
| `ev_evv` | `obj` | `tobj` | 外側の `evv` を読む |
| `ev_take_state` | `obj` | `tobj` | 状態を取り出し、欄を `()` にする |
| `ev_set_state` | `obj`、`tobj` | `unit` | 状態の欄を替える |
| `yielding` | なし | `int` | yield 中か中断中なら 1、そうでなければ 0 |
| `yield_status` | `obj` | `int` | 0: yield していない、1: この evidence 宛ての yield、2: この evidence 宛ての中断、3: ほかの宛先の yield か中断 |
| `yield_to` | `obj`、`tobj` (節の本体)、`tobj` (最初の κ) | `unit` | yield を始める |
| `yield_abort` | `obj`、`tobj`、`tobj` | `unit` | 中断を始める |
| `yield_cont_take` | なし | `tobj` | κ を取り出す (欄は空になる) |
| `yield_cont_set` | `tobj` | `unit` | κ を入れる |
| `yield_finish` | なし | `tobj` | 節の本体を取り出し、宛先の evidence への参照を手放し、yield の状態をなしに戻す |
| `dummy` | なし | `obj` | ダミーの物体 (下の「ダミーの値」) |

- `yielding` と `yield_status` の結果を `int` にするのは、リテラルの `switch` で分けるためである。`Bool` で分けると、EP が配置の表に `Prelude.Bool` を足すことになり、translate の後のパスは配置の表を変えないという規則 (`docs/spec/core-ir.md` の「データの配置」) を崩す
- 基本操作は純粋でないとみなし、縮約は消さない

### 節の呼び出し

- 新しい呼び出しの種類 `Call::Clause { effect, op, callee, args }` を足す。テキストの形は `clause State.get c.3(ev.2, ())` である
- `callee` は in-place の入口のクロージャ、`args` の先頭は evidence (`obj`)、残りは操作の引数である。引数と結果の Repr は、操作の表の `params` と `ret` から取る
- インタプリタでは `apply` と同じに動く。VM とネイティブでは、操作の ABI を知っている間接呼び出しになる
- `mask` は持たない

### 入口の関数の印

- `CoreFn` に `clause: Option<(effect, op)>` を足す。in-place の入口の関数にだけ付け、テキストの形では `internal yields clause State.get fn …` と書く
- 入口の関数は、捕獲の後に evidence と操作の引数を受け、操作の結果を返す。`ev_set_clause` に渡す値としてだけ参照され、`apply` でも直接の呼び出しでも呼ばれない

## EP の変換

### パスの位置と対象

- パイプラインは、translate、`verify_translated`、**evidence**、`verify_evidence`、boxing、`verify_scopes`、contract、`verify_scopes`、Perceus、`verify` の順にする。`Pass` に `Evidence` を足す
- EP は型を読まない。読むのは、関数の `yields` と `clause` の印、`apply` の印、操作の表、Core IR の形と生存解析だけである
- EP は `tail`、`box`、`unbox`、RC の命令を出さない。後のパスの置き場所の規則 (`docs/spec/core-ir.md` の「box の挿入」の最後の項目) のとおりである
- 書き換えるのは、translate が作った関数のうち、`yields` の関数と、`handle`、`perform`、`resume`、`mask` を含む関数である。`yields` でない関数も、row が純粋なまま handle を含むことがある (handle の本体は yield する)。どれにも当たらない関数は変えない
- EP が作る関数 (補助の関数、継続の関数、合流のブロックの関数、入口の関数) は、作るときに最終の形 (yield の確認と遅い道を置いた形) で作り、もう一度は書き換えない

### EP が作る補助の関数

EP は、使うときだけ次の関数をプログラムに1つずつ足す。どれも `yields` の印を立て、`internal` にしない。多くの定義から直接呼ばれるためで、導出の生成器が作る関数を `internal` にしないのと同じ理由である。`internal` の関数を直接呼ぶのはそれを作った定義の中だけ、という規則 (`docs/spec/core-ir.md` の「一様な関数」) を保つ。evidence を受ける引数は `obj`、ほかの引数と `ret` は `tobj` で、どれも一様である。

| 関数 | 引数 | 本体 |
|---|---|---|
| `id$` | `x` | `return x` |
| `compose$` | `κ`、`c`、`x` | `r = apply κ(x)`。yield 中なら、取り出した κ' と c で `compose$(κ', c)` を κ にしてダミーを返す。そうでなければ `apply c(r)` を末尾で呼ぶ |
| `prompt$` | `ev`、`ret`、`x` | 下の「handle と `prompt$`」 |
| `resume$` | `ev`、`ret`、`κ`、`v`、`s` | 下の「再開と状態」 |
| `rewind$` | `ev`、`ret`、`s`、`κ`、`v` | `resume$(ev, ret, κ, v, s)` を末尾で呼ぶ |
| `mask$[effects]` | `κ`、`v` | 下の「`mask`」。`effects` の多重集合ごとに1つ作り、名前に多重集合を書く (`mask$[State]`) |

補助の関数の中の `apply` (κ、c、`ret`、節の本体を呼ぶもの) は、どれも印を立てる。

### yield の地点

- `yields` の関数の中で、末尾でない次の呼び出しを yield の地点とする
  - `yields` の関数への直接の呼び出し (EP が作る関数を含む)
  - 印の立った `apply`
  - `Call::Clause`
- 末尾の位置の呼び出し (`docs/spec/core-ir.md` の「変換の規則」と同じ定義。`let x = <呼び出し>` の後が純粋な `let` と `return x` だけ) は、yield の地点にしない。呼ばれた側の yield は、そのまま呼び出し元へ返るためである
- yield の地点 `let x = <呼び出し>` の直後でブロックを切り、次の形にする

  ```
    let x = call g(a)
    let y: int = prim yielding()
    switch y { 0 -> b_fast, _ -> b_slow }
  b_slow:
    let k0: tobj = prim yield_cont_take()
    let c: tobj = closure f$k1(v1, .., vn)
    let k1: tobj = closure compose$(k0, c)
    let u: unit = prim yield_cont_set(k1)
    return <ダミー>
  b_fast:
    (元のブロックの残り)
  ```

  生きている変数がなければ、`c` は `&f$k1` である
- ブロックを切ると番号が変わるので、EP は関数ごとにブロックを番号の順に作り直し、構造の規則 R1〜R6 を保つ

### ダミーの値

- yield 中に返す値は、呼び出し元が使わずに捨てる
- 関数の `ret` が `obj` か `tobj` なら `prim dummy()` の値を、`int` なら `0` を、`enum` なら `#0` を、`unit` なら `()` を返す。`tobj` にも物体を使うのは、互換の位置を通って `obj` の変数に入ることがあるためである (`ret` が `tobj` の関数の結果を `obj` の変数で受ける形)
- ダミーは、変換と RC の命令に当たる。Perceus は遅い道で死ぬ x に `decref` を置き、box の挿入は呼び出しの結果を受け直す `unbox` と `box` を呼び出しの直後 (yield の確認の前) に置く。box の挿入が足す `f$boxed` は yield の確認を持たないので、`f` が返したダミーを `box` してから返す (`never` の操作で失敗する関数を値として使う形)。そのため、実行時は次を守る。S9 の `eml_rt` も同じ約束を守る
  - `dummy` の値は、`dup` と `decref` で何も起こさない
  - `unbox` は、ダミーを受けても誤りにせず、何かの値を返す。その値は捨てられる
  - `box` は、ダミーを受けたらダミーを返す。`0` と `#0` は普通に箱に入れ、その値は捨てられる
- CEK では、`dummy` をヒープの物体でない専用の値 (`Value` の新しい種類) にする。`box`、`unbox`、`dup`、`decref` はそれをそのまま通す。ヒープに置かないので、`peak_objects` は変わらない
- `never` の操作で失敗する関数を値として使う形を UI テストに足す。`check n = if n < 0 then fail "neg" else n` を `map check [1, -2, 3]` で使い、`fail` の節で `[]` を返す

### yield の確認のない呼び出しの後

- 末尾の位置の呼び出しは yield の確認を持たない。box の挿入の後でも、結果の変換が要る呼び出しは `tail` にならず、呼び出しの後に変換が残る (`f$boxed`、`ret` が `tobj` の関数から `ret` が `int` の関数への呼び出し、`ret` が `tobj` の関数の中のスカラーの `clause`)。yield 中はこの変換がダミーに当たったまま呼び出し元へ返り、κ には入らない
- これが正しいのは、次の2つが成り立つためである。この2つを不変条件とし、`docs/spec/core-ir.md` に書く。後で借用パラメータの降格 (`let r = call g(y)`、`decref y`、`return r`) を入れるときも、この不変条件を保つ
  - yield の確認のない呼び出しの後には、変換 (T3 の後では `box` だけ) と RC の命令と `return` しか続かない
  - κ に入るフレームの関数 (`f$kN`、`compose$` などの補助の関数) は一様なので、再開で届く値はすでに `tobj` で、飛ばした `box` は要らない
- 後で T3 が `ret` を上げても、box の挿入がほかの `return` と同じく変換を入れる

### 継続の関数

- yield の地点 p ごとに、関数 `f$kN` を1つ作る。N は f の中の yield の地点の通し番号で、ブロックと文の順に振る
  - 引数は、p の後で生きている変数 (x を除き、変数の番号の順) と、x である。`ret` は f の `ret` と同じである
  - 印は `internal` と `yields` である
- 本体は、p の直後からの区間の写しである。区間は p の直後から辺をたどり、次のどちらかに当たる所で止める
  - 次の yield の地点 q: 呼び出しと確認を同じ形で置く。遅い道は `f$kq` を κ に足して返し、速い道は `let r = call f$kq(..)` と `return r` にする。後で縮約が `tail` にするので、継続の関数どうしの速い道はフレームを積まない
  - 合流のブロック J (`jump` で入るブロック): 区間の中の `jump bJ(args)` を、`let r = call f$jJ(..)` と `return r` にする。`f$jJ` の引数は、J の入口で生きている変数 (J の引数を除き、変数の番号の順) と J の引数で、`ret` は f と同じである。`f$jJ` は、区間から届く J についてだけ作る
- 区間の根は yield の地点と合流のブロックである。根でないブロック (`switch` の行き先) は入る辺が1本なので、各ブロックは元の関数と、多くとも1つの区間にしか写されない。そのため、写す文の数は元の約2倍に収まる。これとは別に、遅い道が生きている変数を捕獲する。その大きさは yield の地点ごとの生きている変数の数の和で、yield しうる要素を n 個並べたリストのリテラルのような形では n の2乗になる。根ごとに続きを最後まで写す形では、写す文の数そのものが yield の地点の数と関数の大きさの積になる
- 元の関数 f は、速い道をそのまま続けるので、yield の地点の確認と遅い道を足すだけである。論文の bind-inlining は f の速い道に、join-point sharing は `f$jJ` に当たる
- 生きている変数は、今の生存解析 (`liveness.rs`) で求める。`int` と `enum` の捕獲は、ほかのクロージャと同じく box の挿入が `box` する
- `f$kN` は値として参照され、直接も呼ばれるので、box の挿入の今の規則で `f$kN$boxed` が足されることがある

### 継続の組み立て

- κ は、いつも普通のクロージャである。yield を始めるとき、κ は `&id$` である
- 普通のフレームは、上のとおり `compose$(κ, c)` で κ を作り直す。κ を先に呼び、その結果で c を呼ぶ
- 包むフレーム (`prompt$` と `mask`) は、κ を最後の捕獲に持つクロージャ (`rewind$`、`mask$`) で κ を作り直す。再開されると、包むフレームが `evv` を整えてから κ を呼ぶ
- 再開は `apply κ(v)` で、内側のフレームの続きから順に動き、値は外側へ返る
- `once` の継続は `Lin` の値を持ってよく、`multi` の継続は持ち越し規則で `Unr` の値だけを持つ。共有されたクロージャへの `apply` は中身を写すので (`docs/spec/core-ir.md` の「一様な関数」)、`multi` の継続を2回再開しても写すのは捕獲だけである

### handle と `prompt$`

translate の `let r = handle E(init, body) { c_0, .., c_n } return ret` を、次の形にする。

```
  let w0 = prim evv_get()
  let ev = prim ev_new[E](init, w0)
  (操作ごとに) let u_i = prim ev_set_clause[i](ev, <i 番目の in-place の入口>)
  let w1 = prim evv_push(w0, ev)
  let u = prim evv_set(w1)
  let x = apply body(())
  let r = call prompt$(ev, ret, x)
```

- `w0` と `ev` は2回以上使うので、Perceus が `dup` を足す
- `apply body(())` は yield の地点にしない。yield したかは `prompt$` が見る。`call prompt$` は、handle を含む関数が `yields` なら yield の地点である (その関数が `yields` でなければ、その row から外へ出る yield はない)
- `prompt$(ev, ret, x)` は、最初に `evv` を `ev` の外側の `evv` に戻し、`yield_status(ev)` で4つに分ける
  - 0 (yield していない): `st = ev_take_state(ev)` で状態を取り出し、`apply ret(x, st)` を末尾で呼ぶ
  - 1 (`ev` 宛ての yield): `κ = yield_cont_take()`、`c = yield_finish()`、`st = ev_take_state(ev)` を得て、`k = closure resume$(ev, ret, κ)` を作り、`apply c(k, st)` を末尾で呼ぶ
  - 2 (`ev` 宛ての中断): κ を取り出して `drop κ` し、`c = yield_finish()` と状態を得て、`apply c(st)` を末尾で呼ぶ
  - 3 (ほかの宛先): 状態を `ev_take_state(ev)` で取り出し、κ を `rewind$(ev, ret, st, κ)` のクロージャで作り直して、ダミーを返す

### 再開と状態

- `resume$(ev, ret, κ, v, s)` は、`ev2 = ev_renew(ev, s)` で新しい evidence を作り、今の `evv` の先頭に `ev2` を足して `evv` にする。その後 `x = apply κ(v)` を呼び、`prompt$(ev2, ret, x)` を末尾で呼ぶ。deep handler の再接続と、スコープの外での再開に当たる。`ev2` の外側の `evv` は再開した時点の `evv` なので、handle が返った後の再開も、ほかの handler の中での再開も、その場の文脈の上につながる
- `rewind$` は、外側のフレームの yield で handler の外へ出たときに `prompt$` が取り出した状態で、`resume$` を呼ぶ
- 再開した計算の中の操作は、`ev_lookup` で `ev2` を見つける。古い `ev` を持ったまま止まっているフレームはない。`ev` を持つのは、`prompt$` (止まるときは `rewind$` が `ev` の写す部分だけを使う)、`resume$` のクロージャ (同じ)、yield を始める入口 (止まらない)、in-place の節 (本体に yield の地点がないので止まらない) だけだからである
- handler の実行中にその handler 自身の継続を再開する形も、今の CEK と同じ意味になる。次のプログラムでは、`k r 100` の再開が状態 100 の新しい evidence を作り、外側の実行の evidence (状態 1) は変わらない。CEK でも EP でも 101 を表示する。このプログラムを UI テストに足す

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
      | grab () k st -> k (Re k) st
      | get () k st -> k st st
      | put n k _ -> k () n
      | return x _ -> x
  ```

- translate の `Call::Resume { k, v, s }` は `apply k(v, s)` にする。`k` は `resume$` のクロージャ (直接の形) か、`cont$` と `cont$state` のクロージャ (包む形) である。`cont$` と `cont$state` は今のまま残る。`resume` の `mask` は下の「`mask`」の形にする
- `drop k` は、そのままクロージャの破棄になる。継続のクロージャが捕まえていた値は、解放の連鎖で1回ずつ解放される

### `perform` と節の3つの形

translate の `let r = perform E.op(a_1, .., a_n)` を、次の形にする。末尾でなければ `Call::Clause` が yield の地点になる。

```
  let ev = prim ev_lookup[E]()
  let c = prim ev_clause[op](ev)
  let r = clause E.op c(ev, a_1, .., a_n)
```

in-place の入口は、translate が作った節の関数 g から、EP が節ごとに作る。節は次の3つの形に分かれ、EP は Core IR の形から決める。

- **すぐに再開する節**: 次をすべて満たす節である
  - `never` の操作の節でない
  - g の `k` の引数の使いが、末尾の位置の `mask` のない `resume k(v, s)` だけである
  - g のすべての `return` が、そのような `resume` の結果を返す
  - g の本体に、yield の地点になる呼び出しがない。EP は translate の出力の上でこれを決めるので、translate の命令で書くと次のどれもないことである: 末尾でない `perform`、g が `yields` のときの `handle`、`mask` 付きの呼び出し、最後の末尾の `resume` のほかの `resume`、`yields` の関数への末尾でない直接の呼び出し、印の立った末尾でない `apply`

  入口の関数 `g$entry` (`clause` の印) は g の写しである。引数は、捕獲、evidence `ev`、操作の引数の順にし、g の `k` と状態の引数を除く。状態は入口の先頭の `ev_take_state(ev)` で得る。末尾の `let r = resume k(v, s)` と `return r` を、`ev_set_state(ev, s)` と `return v` に替える。`yields` の印は g の印を写す。g は使われなくなるので消す

  本体は yield しないので、`evv` を替えずに `perform` の場所の `evv` のまま動かしてよい。g が `yields` でないときは、本体に `handle` や `mask` 付きの呼び出しが残りうるが、それらが呼ぶ関数は yield しない。健全性の不変条件 (`docs/spec/effects.md` の「健全性」) から、その中の操作はその中で設けた handler に届くので、外側の `evv` の違いは見えない
- **再開する節** (`once` と `multi` の操作のうち、上に当たらない節): 入口の関数は、捕獲、`ev`、操作の引数を受け、`c = closure g(捕獲, 操作の引数)` を作り、`yield_to(ev, c, &id$)` を呼んで、ダミーを返す。`prompt$` が後で `apply c(k, st)` を呼ぶ。外のエフェクトを呼ぶすぐに再開する節もここに入り、節の本体は handle の外側の `evv` で動く
- **`never` の節**: 入口は同じ形で `yield_abort(ev, c, &id$)` を呼ぶ。`prompt$` が後で `apply c(st)` を呼ぶ。入口の `ret` は `obj` で、`prim dummy()` を返す。`never` の操作の `clause` の束縛は、`OpRet::Never` なので位置と比べない。その値はダミーで、上の「ダミーの値」の約束で安全である
- handle の場所では、節の値 c_i がどの関数のクロージャかを知る必要がある。translate は節を `&g` か、同じ関数の中の `closure g(..)` で渡すので、EP はそれを読む。そうでない形は内部の誤りとしてパニックする
- `ev_set_clause` には、入口の関数のクロージャ (捕獲がなければ関数の値) を渡す。捕獲は節の捕獲をそのまま写す。再開する節と `never` の節の入口が作る c も、捕獲も操作の引数もなければ `&g` にする (`closure` は引数を1つ以上とる)
- 入口の関数、`f$kN`、`f$jJ` は、元の関数 f の定義に属する内部の関数とみなす。`f$kN` と `f$jJ` は同じ定義の別の継続の関数から直接呼ばれるが、「内部の関数を直接呼ぶのはそれを作った定義の中だけ」という規則の「定義」は、EP がその定義から作った関数を含む

### `never` の中断

- 中断の yield も、途中のフレームが普通の yield と同じく κ を組み立てる。`prompt$` が κ を `drop` すると、クロージャの解放の連鎖で、捕まっていた値が1回ずつ解放される。`File` はこの解放で閉じる
- 途中のフレームは、生きている `Lin` の値に `decref` を置かない。生きている値は継続のクロージャに捕まるためである。遅い道で `decref` されるのは、yield の地点の結果 x (ダミー) だけである (下の「S8 への申し送り」)
- 中断の yield でクロージャを作らずに済ませる形は、「処理系」の項目に移す

### `mask`

translate の `mask` 付きの呼び出し (直接の呼び出し、`apply`、`resume` から作った `apply`) を、次の形にする。

```
  let w0 = prim evv_get()
  let w1 = prim evv_mask[effects](w0)
  let u = prim evv_set(w1)
  let x = <mask を外した呼び出し>
  let y: int = prim yielding()
  switch y { 0 -> b_fast, _ -> b_slow }
b_slow:
  (κ を mask$[effects](κ) で包み、その上で f$kN を compose$ で足して、ダミーを返す)
b_fast:
  let u2 = prim evv_set(w0)
  (元のブロックの残り)
```

- 遅い道は、κ を `compose$(mask$[effects](κ), f$kN)` にする。`mask$` が内側のフレームの再開のときに `evv` を整え、`f$kN` が f の残りを続ける
- `f$kN` の区間は、`evv_set(w0)` の後から始まる。`w0` は区間で生きていないので捕獲しない。再開した後の `evv` は、外側の包むフレームが作り直した `evv` で、`mask$` が戻るときにそれへ戻すので、f の残りはその `evv` で動く
- `mask$[effects](κ, v)` は、再開されたとき、`w = evv_get()` を得て、`evv` を `evv_mask[effects](w)` にし、`r = apply κ(v)` を呼ぶ。yield していなければ `evv` を `w` に戻して r を返す。yield 中なら、もう一度 `mask$` で包んでダミーを返す
- `mask` のラベルは今の row の handle できるラベルなので、`mask` を持つ関数はいつも `yields` である
- `mask` 付きの末尾の位置の呼び出しの後には `evv_set(w0)` が続くので、末尾呼び出しにならない

### 不変条件と末尾呼び出しの保証

- 呼び出しが普通に返ったとき、`evv` は呼び出しの前と同じである。`evv` を替える所 (handle の `prompt$`、`resume$`、`mask`) が、戻るときに必ず元に戻すためである
- yield 中の `evv` には意味がない。再開のときに、包むフレームが外側から順に `evv` を作り直す
- `mask` 付きの呼び出しは、戻りで `evv` を戻すのでフレームを残す。`mask` は見える別々の handler を飛ばすので、継続の上で `mask` が残すフレームは handler の数以下である (`docs/spec/effects.md` の「健全性」の最後の段落と同じ議論)。`prompt$` と `resume$` のフレームは、今の CEK の handler フレームに当たる
- そのため、末尾呼び出しの保証 (`docs/spec/core-ir.md` の「変換の規則」) は今と同じ意味で保てる。段の終わりに、保証の文と、その理由の段落を次のように書き直す
  - 輪の辺は、直接の呼び出しなら呼ばれる関数へ、`apply` ならどの関数へも向きうるとみなす。`clause` の辺は輪に乗らない。入口の関数 (すぐに再開する節、再開する節、`never` の節) は末尾呼び出しを持たないためである
  - `mask` 付きの呼び出しは保証の外に置く。そのフレームは handler の数で抑えられる (上のとおり)
  - 補助の関数 (`compose$`、`prompt$`、`resume$`、`rewind$`) の末尾呼び出しは、どれも `tobj` から `tobj` への呼び出しなので、いつも `tail` になる
  - 今の理由の段落の「T3 の後に結果が互換でない末尾の位置の呼び出しは、`ret` が `tobj` の関数からスカラーを返す関数への直接の呼び出しだけ」に、スカラーを返す操作の `clause` を足す。`clause` の辺は輪に乗らないので、議論の結論は変わらない

### S8 への申し送り

- evidence と `evv` のリストは、`evv` と局所変数から共有される入れ物で、`Lin` の状態は `ev_take_state` と `ev_set_state` で出し入れする。そのため、evidence と `evv` の変数の Kind の印は `unr` である。`Lin` の物体の RC は 1、という S8 の不変条件の対象外である
- yield の地点の結果 x は、遅い道ではダミーで、Perceus がそこに `decref` を置く。x の型が `Lin` (`File` を返す呼び出し) なら、S8 の V1 (`unr` でない変数に `decref` を置かない) とぶつかる。S8 で、この `decref` を V1 の例外にするか、EP が遅い道で x を別の形で捨てるかを決める。ロードマップの S8 の論点に足す

## 後段のパス

### box の挿入

- `Call::Clause` は、操作の表の `params` と `ret` を相手に、直接の呼び出しと同じく変換を入れる。先頭の evidence は `obj` である。`never` の操作の `clause` の束縛は、今の `never` の `perform` と同じく受け直さず、その使いにも変換を入れない
- `prim` の引数と結果は、表の Repr を相手に、直接の呼び出しと同じく変換を入れる
- `clause` の印の関数は、一様化の対象から外し、操作の ABI に合わせる。捕獲は `tobj` と互換、evidence は `obj`、残りの引数と `ret` は操作の表の Repr (`never` の操作は `obj`) である。合わない引数は、その場で一様にする今の手順と同じく、入口で `unbox` (または `box`) を置いて受け直す。`ret` が合わなければ、`return` で変換する
- T3 は、`clause` の結果を操作の `ret` とみなす。`never` の操作の `clause` は見ない (今の `never` の `perform` の規則を移す)

### 縮約

- `clause` も末尾呼び出しにできる。結果は操作の `ret` で、`never` の操作ならどの `ret` とも互換とする
- `prim` は純粋でないので消さない

### Perceus

- 新しい規則は足さない。`prim` は extern と同じく引数をすべて消費し、所有した結果を定義する。`clause` は呼び出しとして退避 (`saved`) を持つ

### verifier

- 新しい段 `verify_evidence` を、EP の後にかける。変換の段 (`verify_translated`) の検査に加えて、次を確かめる
  - `handle`、`perform`、`resume` がなく、`mask` がどの呼び出しにもない
  - `prim` の引数の数
  - `clause` の操作がエフェクトの表にある。引数の数が「1 + 操作の引数の数」である
  - `ev_set_clause[op]` に渡す値が `&g` か、同じ関数の中の `closure g(..)` なら、g の `clause` の印が、同じ関数の中で evidence を定義した `ev_new[E]` の E と、op に合う
  - `clause` の印の関数が、`ev_set_clause` の引数のほかで値として参照されず、直接も呼ばれない
- 範囲の段と所有の段に、`prim` と `clause` の境界の検査を足す。文言は今の `perform` の形に合わせる (``argument 1 of a clause of `Counter.put` is `n.1` (tobj), but the operation takes int``、`` `t.0` (int) is bound to a clause of `State.get`, which returns tobj ``、``a tail clause of `Counter.get` returns int, but this function returns tobj``、``argument 0 of `prim ev_new` is 0, but the primitive takes tobj``)。`clause` の印の関数の引数と `ret` が操作の表と互換であることも確かめる (`` `g$entry` is a clause of `Counter.put`, but its parameter 2 is tobj ``)
- 段の終わりに、範囲の段と所有の段から `handle`、`perform`、`resume`、`mask` の規則を消す。translate の出力は今のままなので、変換の段の規則は残す

## CEK とランタイム

- 機械に、`evv` と yield の状態を足す。始めは、空の `evv` と yield なしである。終わったときに `evv` が空で、yield の状態がなしであることを確かめる (`debug_heap` のリークの検査の前提)
- `eml_runtime` の `Payload` に、入口の表 (エフェクトの番号と入口の欄)、evidence (入口の表、状態、外側の `evv`)、`evv` のリストの節点 (evidence と次の節点) を足す。`children` はどれもたどり、`debug_heap` のリークの数え方もほかの物体と同じにする
- 基本操作は `eml_interp` の新しいモジュール `prims.rs` にまとめる。`clause` の呼び出しは `apply` と同じに動く
- `dummy` は上の「ダミーの値」のとおりにする
- 段の終わりに、次を消す
  - `eml_runtime` の `Frame::Handler`、`Frame::Mask`、`Attachment`、`Payload::Continuation`、継続のための `take_or_copy` と区間の複写 (`copy_segment`)
  - `eml_interp` の `find_handler`、`perform`、`resume`、handler の連鎖 (`handlers`)
  - `Frame` には `Return`、`Apply`、`Root` が残る

## `RunStats`

- `handler_visits` を消す。CEK のフレームをたどる回数で、EP の後は意味を持たない
- 次を足す
  - `evidence_steps`: `ev_lookup` と `evv_mask` がたどったリストの節点の数
  - `yield_frames`: `yield_cont_set` を実行した回数 (κ に足したフレームの数)
  - `yields`: `yield_to` と `yield_abort` を実行した回数
- 完了の条件を確かめる scaling テストを `eml_interp/tests/scaling.rs` に足す
  - 末尾でない再帰の深さ d (たとえば 10 と 1000) の底で、すぐに再開する操作 (状態の `get` と `put`) を同じ回数行う。`evidence_steps`、`yields`、`yield_frames` が d によらないことを確かめる
  - 同じ深さの底で、再開する操作を1回行う。`yield_frames` が d に比例することを確かめ、方式の性質としてテストに書く

## 差分テストと段の終わり

- 開発中は、lower に EP を飛ばす切り替えを持たせる。`eml_core_ir` に一時的な関数 `lower_without_evidence` を足し、`eml_cli::Session` の `compile_with` から呼ぶ一時的なメソッドを足す
- 一時的な UI の差分テスト (`crates/eml_cli/tests/ui_evidence.rs`、`tests/main.rs` で宣言する) は、`ui.rs` のテストの読み込みと分類を共有して、`tests/ui/run` と `tests/ui/run-fail` のすべてのテストを、EP を通す経路と通さない経路の2通りで実行する。標準出力と実行時エラーの表示が一致することを確かめる。どちらも `debug_heap` を付ける。RC の回数と `RunStats` は比べない。`run-fail` の表示は extern の位置を持つので、継続の関数で名前が変わっても表示は変わらない
- 段の終わりに、切り替え、一時的な関数とメソッド、差分テストを消し、EP を常に通す。消すものは上の「verifier」と「CEK とランタイム」に書いた

## テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。

### 成否の変更

boxing より後の段に `handle`、`perform`、`resume`、`mask` を含む手書きの IR を流すテストと、消す実行時の仕組みを直接使うテストは、消すか形を変える。

| テスト | 変更 |
|---|---|
| `eml_core_ir` `verify.rs` `the_operands_of_uniform_calls_are_compatible_with_tobj` | `perform`、`resume`、`handle` のケースを消し、`clause` と `prim` のケースを足す |
| 同 `the_results_of_uniform_calls_are_bound_to_variables_compatible_with_tobj` | 同上 |
| 同 `the_result_of_a_tail_call_is_compatible_with_the_function` | 同上 |
| 同 `the_uses_of_a_never_perform_binder_are_not_compared` | `never` の操作の `clause` で書き直す |
| 同 `a_never_perform_has_no_result_to_compare` | 同上 |
| 同 `handlers_operations_and_resume_are_calls` | 所有の段で `handle` と `resume` を呼び出しとして確かめるテストなので消す |
| 同 `the_translated_level_rejects_tail_calls` | 範囲の段が `tail perform`、`tail resume`、`tail handle` を受け付けることを確かめる部分を消す |
| 同 `a_handler_has_a_clause_for_each_operation`、`a_clause_receives_the_arguments_k_and_the_state_after_its_captures`、`a_clause_of_a_never_operation_receives_the_arguments_and_the_state`、`a_return_clause_receives_the_value_and_the_state_after_its_captures`、`a_clause_outside_its_scope_is_rejected_before_its_parameters_are_counted`、`perform_names_an_operation_of_its_effect`、`a_perform_must_agree_with_the_effect_on_resuming`、`a_mask_must_name_known_effects_in_order`、`a_mask_may_name_one_effect_twice`、`a_mask_on_handle_or_perform_is_rejected` | 規則は変換の段に残るので、変換の段で確かめる形に移す。所有の段か範囲の段でしか起きない誤りを確かめるケースは消す。ヘルパー `handler_program` も変換の段に合わせる |
| `contract.rs` `every_kind_of_call_returned_at_the_end_becomes_a_tail_call` | `perform`、`handle`、`resume`、`mask` のケースを消し、`clause` のケースを足す |
| 同 `a_never_perform_returned_at_the_end_becomes_a_tail_call_in_any_function` | `never` の操作の `clause` で書き直す |
| `perceus.rs` `a_masked_call_keeps_its_mask_through_the_passes` | EP の後に `mask` は残らないので消す。`evidence.rs` に `mask` の包みのテストを足す |
| `eml_interp` `run.rs` `a_mask_skips_outer_handlers_only` と、それが読む `tests/ir/mask.core` | 手書きの `handle`、`perform`、`mask` の IR を CEK で動かすので、EP の出力の形の IR に書き直す |
| `closures.rs` `an_inner_handle_returns_through_each_resumption_of_an_outer_multi_operation` | 同上 |
| `eml_runtime` `heap/tests.rs` の `a_handler_frame_releases_its_clauses_and_the_rest_of_the_continuation`、`releasing_a_continuation_stops_at_its_detached_handler`、`a_mask_frame_releases_the_rest_of_the_continuation`、`take_or_copy_takes_a_unique_continuation_without_copying`、`take_or_copy_copies_the_segment_of_a_shared_continuation`、`copying_a_segment_keeps_the_resume_address_of_a_return_frame`、`copying_a_segment_copies_its_mask_frames`、`copying_a_long_segment_does_not_overflow_the_stack`、`an_attached_handler_releases_its_state`、`copying_a_segment_shares_the_state_of_an_attached_handler`、`copying_a_segment_remaps_its_handler_chain`、`a_chain_link_out_of_the_segment_breaks_the_segment`、`an_inner_link_out_of_the_segment_breaks_the_segment`、`an_inner_link_to_a_chain_frame_that_is_not_the_innermost_breaks_the_segment`、`a_mask_whose_outer_is_not_a_chain_frame_breaks_the_segment` | 消す仕組みのテストなので消す。evidence と `evv` の物体の解放のテストを足す |
| 同 `take_or_copy_refuses_data_strings_and_files` | 継続を使う部分を消す。`take_or_copy` がクロージャのために残るなら、その部分は残す |

### 期待値の変更

- handler を使うプログラムと、`yields` の instance を含むプログラムの、translate より後の段の Core IR のダンプ (`boxing.rs`、`contract.rs`、`perceus.rs`、`translate.rs` と `instances.rs` の translate より後を見るテスト)
- `<e>` の付く instance の名前を含むダンプと、`instances.rs` の名前の期待値
- translate の出力のダンプのうち、`yields` の印と `apply` の印が付く関数と、yield しうる矢印の後で分けた `apply`
- テキストの形の `effect` の宣言が変わるので、`effect` を含むすべてのダンプと手書きの IR。手書きの IR の入力の書き換えは機械的な追随で、`text.rs` の `an_operation_needs_its_arity_after_the_last_slash` の文言は期待値の変更である
- `eml_interp/tests/bench.rs` の `RunStats` のスナップショット (`handler_visits` が消え、新しい指標と RC の回数が変わる)
- `handler_visits` を使う `scaling.rs` のテスト (`a_program_without_operations_or_strings_does_no_counted_work`、`every_operation_visits_at_least_its_handler`、`concatenation_writes_the_bytes_of_both_sides`、`a_perform_under_one_handler_visits_only_that_handler`、`a_perform_passes_an_inner_handler_of_another_effect_once`、`a_perform_in_a_masked_callback_skips_the_inner_handler_once`) を、`evidence_steps` と `yields` で書き直す
- UI テストの出力は変わらない (差分テストで確かめる)

### 新しいテスト

- `crates/eml_core_ir/tests/evidence.rs`: EP のダンプ。yield の地点、継続の関数と合流のブロックの関数、handle と `prompt$`、節の3つの形、`mask` の包み、`yields` でない関数を変えないこと、row のビットで純粋な instance に EP が何も足さないこと
- `text.rs`: `prim`、`clause`、`yields` と `clause` の印、`yields apply`、操作の Repr、`<e>` を含む名前の読み書き
- `verify.rs`: `verify_evidence` の文言と、`prim` と `clause` の境界の検査の文言
- `eml_types` `instantiations.rs`: `rows` の記録
- `eml_interp`: 基本操作の動作、上の scaling テスト
- UI テスト `tests/ui/run/effects/resume_inside_own_handler.em`: 上の「再開と状態」のプログラム
- UI テスト `tests/ui/run/effects/over_application_yields.em`: 1つの `apply` で、yield する関数に余った引数を渡す形。`adder : Int -> <Ask> (Int -> Int)` を `h 1 2` で呼ぶ
- UI テスト `tests/ui/run/effects/never_in_function_value.em`: 上の「ダミーの値」の、`never` の操作で失敗する関数を値として使う形
- UI テスト `tests/ui/run/effects/continuation_in_outer_state.em`: 上の「evidence」の、内側の handler の継続を外側の handler の状態に入れる形。`debug_heap` がリークを報告しないことを確かめる
- UI テスト `tests/ui/run/effects/operation_row_variable.em`: 操作のシグネチャの row 変数を節の中のラムダの型に使う形 (`run : (Unit -> <e> Int) -> Int` の節で `fn () -> f () + 1` を作る)

## 文書の更新

段の終わりに1回で行う。

- `spec/core-ir.md`: EP のパス、基本操作の表、`clause`、`yields` と `clause` の印、`apply` の分け方と印、操作の表の Repr、row のビットの単相化と名前、`never` の操作の規則の移動、ダミーの値の約束、末尾呼び出しの保証の文、内部の関数の規則と補助の関数、インタプリタの節 (`handle`、`perform`、`resume`、`mask` の動作を EP の実行時のモデルに替える)、verifier の4つの段
- `spec/effects.md`: 継続を写す説明、handler フレームの状態の説明 (37 行目、127 行目、134 行目のあたり)、健全性の段落の `Mask` フレームの議論を、EP の形に合わせる
- `spec/expressions.md`: パラメータ付き handler の状態を handler フレームに置く説明
- `spec/types.md`: 参照ごとの row の具体化の記録
- `spec/runtime.md`: handler の連鎖の節を、evidence と `evv` の物体に替える。「実行の API」の `handler_visits`
- `implementation/architecture.md`: 「継続のフレーム」を EP の実行時の説明に替える。パイプラインと `Pass` の一覧、`eml_types` の `Instantiation` の説明
- `implementation/testing.md`: Core IR のテキストの形と、scaling テストの説明
- `implementation/status.md`: `perform` の費用の説明。`bench/run.sh` の結果
- `implementation/benchmarks.md`: `bench/run.sh` の結果
- `README.md`: `future/evidence-passing.md` への2つのリンク
- `overview.md`: 状態を handler フレームに置くという説明
- `future/evidence-passing.md`: 決まった部分を移したうえで消す。残る項目は `future/roadmap.md` の「処理系」と `future/multicore.md` に移す。残る項目は次のとおりである
  - 探索の場所ごとのキャッシュと、閉じた row の静的な添字
  - 中断の yield でクロージャを作らない形
  - 外のエフェクトを呼ぶすぐに再開する節をその場で実行する形 (under)
  - `mask` 付きの呼び出しのたびに `evv_mask` が確保しないようにすること
  - 末尾の位置の `mask` 付きの呼び出しで、x を返すだけの `f$kN` を作らないこと
  - T3 で `ret` を上げた関数の遅い道で、ダミーの `0` を `box` しないこと
  - 継続を別のコアに移すときの共有の印付け
- `future/multicore.md`: evidence passing の設計を指す所と、`par` の子に渡す evidence の説明
- `future/roadmap.md`: S7 の節を消す。S8 の節に「S8 への申し送り」を足し、EP の後の記述をこの設計に合わせる。S9 の「evidence のベクタ」と、S10 の `evidence-passing.md` への参照を直す
- `CLAUDE.md`: Core IR のパスの列、verifier の段、一様な位置と T3 と `never` の説明、`Frame::Root` の説明、`RunStats` の作業の回数の数と名前
- 消す節や文書を引くコード (テストを含む) のコメント: `eml_runtime` の `heap.rs`、`eml_interp` の `runtime.rs` と `effects.rs`、`eml_types` の `kind/mod.rs` と `carry.rs`、`eml_core_ir` の `lib.rs` (`Call::Handle` と `Call::Perform` の説明)、`eml_types/tests/linearity.rs` のうち、消える節や handler フレームの状態を指すもの (`citations.rs` の検査が文書の参照を見つける)

## 作業の順序

各手順の終わりに、全テストが通る。

1. 型検査の `Instantiation` に `rows` を足す。translate の instance の鍵に row のビットを入れ、`yields` の印と `apply` の印と分け方、操作の表の Repr、テキストの形を足す。EP はまだないので、変わるのはダンプの名前と印と `apply` の分け方だけである
2. `Rhs::Prim`、`Call::Clause`、`clause` の印とテキストの形。verifier の4つの段の `prim` と `clause` の規則、box の挿入と T3 と縮約の `clause` と `prim` の規則、`clause` の印の関数の ABI。ランタイムの evidence と `evv` の物体、CEK の基本操作 (`prims.rs`)。手書きの IR を全パスに流すテストで、基本操作と後段のパスの規則を確かめる
3. EP のパス。最初は切り替えで飛ばす。`evidence.rs` のテストと一時的な差分テストを入れ、全 UI テストで出力が一致するまで進める。新しい2つの UI テストを足す
4. `RunStats` の新しい指標と scaling テスト
5. EP を常に通し、切り替え、差分テスト、エフェクトを直接扱う部分を消す。文書を移し、`bench/run.sh` の結果を記録する

## 採らなかった案

- **閉じた row の静的な添字と open (Koka と同じ形)**: 閉じた row の `perform` を定数の添字で引ける。しかし、eml ではトップレベルの関数の row がすべて閉じているので、handler の下での呼び出しの多くが open の包みを通る。包みは部分ベクタを確保し、戻りでベクタを戻すので末尾呼び出しを失う。Core IR に row とラベルの順序を持たせる必要もある。動的に探す費用は handler の数に比例する数命令で、ネイティブと VM のどちらでも目立たないと見ている。目立ったときは、`evv` が書き換えられないことを使って `perform` の場所ごとのキャッシュを足すか、静的な添字に替える。どちらも EP と実行時の中に閉じ、型検査と translate は変わらない
- **`evv` を関数の引数で渡す形**: `mask` が末尾呼び出しを保てる。しかし、関数値の呼び出しの規約が「yield しうるか」で分かれ、関数値として使う関数に evidence の引数を足す一様化が要る。今の `mask` が残すフレームは handler の数で抑えられるので、文脈に持つ形にした
- **handle 1回につき evidence を1つにし、状態を yield と再開で出し入れする形**: 再開で evidence を作らずに済む。しかし、handler の実行中にその handler 自身の継続を再開すると、2つの実行が状態の欄と外側の `evv` の欄を共有して壊し、外側の `evv` がその evidence 自身を指す参照の輪もできる (上の「再開と状態」の例)
- **本体が yield する節もその場で実行する形 (under)**: 外のエフェクトを呼ぶ節も継続を作らずに済む。しかし、節の実行中に `evv` を外側に替え、yield したら替え直す包みが要る。再開で evidence を作り直すと、包みと節の本体が古い evidence を持ち続けるので、探し直す仕組みも要る。外のエフェクトを呼ぶ節は少なく、再開する節として正しく動くので、後の最適化に回す
- **中断の yield の経路に drop の命令を生成する形**: 継続のクロージャを作らずに済む。しかし、Core IR は Kind を持たないので、`Lin` の値を `decref` で解放することになり、S8 の V1 と合わない
- **操作の Repr を継続の再開と yield の引数にも通す形**: 継続ごと、節ごとに呼び出しの規約が要る。効くのは再開する操作だけで、すぐに再開する操作 (状態の `get` と `put`) は in-place の入口の境界だけで効く
- **box の挿入が、yield の地点の結果の変換を確認の後に置く形**: ダミーが変換に当たらずに済む。しかし、box の挿入に EP の形を知る特別な規則が要る。ダミーの側で変換と RC の命令を受け止める約束にした
