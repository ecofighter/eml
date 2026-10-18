# S13e 特殊化と融合と reuse の設計

## 目的と範囲

S13e の設計である。ロードマップ (`docs/future/roadmap.md`) の S13e は「`Iter` の状態機械への特殊化」の1項目だったが、2026-10-18 の議論で、既知の閉包への特殊化、handler への特殊化、インライン化と融合、Perceus の reuse analysis の4つの作業に広げた。この文書は4つの作業をまとめて決める。計画は作業ごとに立てる。

S13e で速くしたい形は次の2つである。

- 末尾再帰の生産者を `Iter.generate` で包み、`Iter.each` や `Iter.fold` で引く形 (`tests/ui/run/std/std_iter.em` の `nums` など)
- 熱いループの中で、高階関数 (`List.map`、`List.filter`、`List.fold`、`Yield.fold` など) に閉包を渡す形

S13e-2 の特殊化は、`Iter` 専用の見分け方にしない。意味の上の形 (届く先が静的に分かる `perform` と、その節の形) で見分ける。将来の `Async` のスケジューラ (`docs/future/multicore.md`) が、待っているタスクの `k` を返す形 (Rust の `Poll` に近い形) で書かれれば、同じ規則で特殊化できるようにするためである。`Async` のライブラリそのものは S13e に入れない。

次のものは範囲に入れない。

- 定数の引数についての特殊化、純粋な extern の定数畳み込み、Repr が同じ instance をまとめること (ロードマップの「Core IR の上の最適化パス」の残り)
- 借用パラメータ (ロードマップの「Perceus の最適化」の残り)
- 末尾でない再帰の生産者 (木の走査など) の特殊化。フレームごとに継続を作って渡す CPS の拡張が要る。`Async` を作るときに測って決める
- 外側の操作を起こす節の yield を減らすこと (ロードマップの「evidence passing の最適化」の under の項目)

## 2026-10-18 に決めたこと

次のことは、ユーザーが決めたか了承した。

- S13e-2 の特殊化は、`ctl` の節が `k` を呼ばずに handle の結果に入れて返す形と、すぐに再開する節 (状態つきを含む) を対象にする。見分け方は std の印でなく、Core IR の形で決める。届く先が静的に分からないときと、経路に末尾でないフレームがあるときは、今の道に残す
- 既知の閉包への特殊化と、Perceus の reuse analysis を、どちらも S13e で入れる
- インライン化と、既知のコンストラクタの jump threading と、`con` の値の形についての特殊化 (融合) も S13e で入れる
- 4つの作業に分け、S13e-1 (既知の閉包)、S13e-2 (handler)、S13e-3 (インライン化と融合)、S13e-4 (reuse) の順に進める
- 下の「計測 (S13e の前)」の表のプログラムを `bench/` に入れ、手で特殊化した版と比べる。手で特殊化した版はラムダの本体まで埋め込んでいるので、S13e-1 (インライン化なし) では、要素ごとの直接の呼び出しが1回残る。そのため、S13e-1 では `RunStats` の確保と `box` / `unbox` の回数が手で特殊化した版と同じになることを確かめ、RC の回数と命令数が同じになることは S13e-3 で確かめる (条件を2つに分けることを、レビューの後にユーザーが決めた)。RC を S13e-3 に回すのは、計画を書くときの計測で、直接の呼び出しが要素の所有権を受け取るので、呼び出しの後も要素を使う形 (`filter` の `x :: acc`) では1要素ごとに `dup` が残ると分かったためである。注文の集計を1万件にした検査モードの `RunStats` は、書いたままの版が `rc_increments` 10,000、`boxes` 22,033、手で特殊化した版が 0 と 4,011、S13e-1 の出力を手で再現した版 (既知の関数を直接呼ぶ形) が 10,000 と 4,011 だった。本体を埋め込めばフィールドを借りて読むだけになるので、インライン化 (S13e-3) で揃う (ユーザーが了承した)
- `Yield` で書いた生産者を `Iter.generate` で引く形を、`bench/` のプログラムとして足して測る。比べるため、同じ合計を直接のループと push の形 (`Yield.fold`) でも測る

## 計測 (S13e の前)

2026-10-18 に、コミット 3117d090 の release ビルドの `eml run` (VM の速いモード) を `/usr/bin/time -l` で測った (Apple M4)。命令数は、空のプログラム (4,070万) の分を引いた実行の分である。プログラムは使い捨てで、リポジトリには入れていない。手で特殊化した版は、既知の閉包を渡す高階関数を、閉包を埋め込んだ専用の関数に書き換えたもので、出力が同じことを確かめた。

| プログラム | 書いたまま | 手で特殊化 | 減った割合 |
|---|---:|---:|---:|
| 注文の100万件を `List.filter` / `map` / `fold` にラムダを渡して集計 | 34.4億 (0.11 s) | 24.8億 (0.08 s) | 28% |
| push の形: `Yield.fold` にラムダを渡して100万要素を合計 | 21.4億 (0.09 s) | 11.4億 (0.04 s) | 47% |
| `Iter.generate` の後に `Iter.filter` / `map` / `fold` (100万要素) | 130億 (0.53 s) | 119億 (0.48 s) | 8% |
| 30万件の木を総称な `fold` とコールバックで畳む | 55.4億 (0.24 s) | 53.7億 (0.23 s) | 3% |
| `logstat` (31 MB のログ) の `upsert` と `sort_by` | 357億 (1.81 s) | 352億 (1.81 s) | 1.4% |
| 同じく、`use` で handler に渡す本体も特殊化 | | 352億 | 0% |

同じ100万要素の合計を、直接のループで書くと3.3億 (0.01 s)、`Iter.generate` と `Iter.fold` で書くと95.1億 (0.35 s) だった。

1,000要素の `Iter.generate (nums 1000)` を `Iter.fold` で合計すると、`RunStats` は `yields: 1000`、`evidence_steps: 1000` だった。最終の LIR では、`yield` の1回ごとにヒープの確保が10回ある (`ev_renew`、`evv_push` のリストのセル、節のクロージャ、`go$k0`、`compose$`、`resume$`、`cont$`、`Iter`、組、`Some`)。そのうえ `prompt$` で handler を外し、`resume$` で付け直す。生産者が作る κ は、恒等関数との合成 `compose$(id$, go$k0)` である。`Int` は即値なので、`box` は確保にならない。

この計測から分かったことは次のとおりである。

- S13e-1 は、熱いループの中で高階関数に閉包を渡す形で、3割から5割効く。閉包の確保、`box` / `unbox`、`apply` が消えるためである
- `Iter` の形では `generate` の費用が大半なので、S13e-1 だけでは8%しか減らない。S13e-2 と S13e-3 の担当である
- `logstat` の費用の大半は閉包の外 (文字列の処理、木の比較、レコードの更新) にある。`use` で渡す本体は1回しか呼ばれないので、特殊化しても効かない。レコードの更新と木の作り直しは S13e-4 の reuse の相手である
- push の形は、手で特殊化した後も直接のループの3.5倍かかる。残りは、要素ごとの evidence の検索と節の呼び出しで、S13e-2 がすぐに再開する節をその場に展開して縮める

## 作業の分け方と完了の条件

| 作業 | 中身 | 完了の条件 |
|---|---|---|
| S13e-1 | 最適化パスの骨組みと、既知の閉包への特殊化 | 上の表の上から4つのプログラムと `logstat` の形を `bench/` に入れ、`RunStats` の確保 (下の「`RunStats` に足す欄」)、`boxes`、`unboxes` が、手で特殊化した版と同じになる。`bench/run.sh` の命令数を記録する |
| S13e-2 | handler への特殊化 | `Iter.generate` で引く形の `yields` が0になる。push の形の `evidence_steps` が0になる。直接のループとの実時間の差を記録する |
| S13e-3 | インライン化、既知のコンストラクタの jump threading、`con` の値の形についての特殊化 | `Iter.generate` で引く形と `Iter.map` / `filter` を重ねた形で、1要素ごとの確保がなくなる (下の「`RunStats` に足す欄」で確かめる)。ロードマップの S13e の完了の条件「1要素ごとの yield と確保がなくなる」は、ここで満たす。S13e-1 の表のプログラムが、`RunStats` の RC の回数と `bench/run.sh` の命令数で、手で特殊化した版に届く |
| S13e-4 | Perceus の reuse analysis | `list.em`、`tree.em`、`logstat` の形のプログラムで reuse の回数が出る。`logstat` を `bench/scripts.sh` で測り、S13e-3 の後の値と比べて記録する |

S13e-2 の push の形について、直接のループに対する倍率の目標は置かない。差を記録し、残りの理由を `benchmarks.md` に書く (私の判断)。

## 最適化パスの共通の形

### 置き場所

Core IR の上に、新しいパス `optimize` を置く (名前は私の判断)。順は translate、`optimize`、EP である。`eml_core_ir::Pass` は `Translate`、`Optimize`、`Evidence` の3つになり、`core_until` はどのパスの直後でも止められる。

EP の前に置くのは、次の3つのためである。

- EP が見る `apply` と `yields` の印が先に減る
- S13e-2 は、`handle` と `perform` が残っている段で動く必要がある
- `docs/spec/core-ir.md` の「パス」の「後のパスの置き場所」(呼び出しを作るか行き先を変えるパスは、translate と box の挿入の間に置く) に合う。`optimize` は `tail`、`box`、`unbox`、RC の命令を出さない

S13e-1 で、関数ごとの作業リストを持つ1つのパスを作る。S13e-2 と S13e-3 は、そのパスに規則を足す。インライン化の後で新しい既知の閉包や既知の handle が現れるので、別々のパスに分けると往復が要るためである。作業リストは、ロードマップの「Core IR の上の最適化パス」が書くとおり、枝の部分木を置き換える形の2乗の時間を避ける。

### 鍵と名前

特殊化した複製は、鍵から決めた名前を持つ。鍵が同じなら同じ複製を使い回す。

- 名前は鍵だけから決める。`docs/spec/vm.md` の「追記」の、名前が同じ関数はいつも同じコードになるという規則を守るためである。REPL は入力ごとにプログラム全体を lowering し直し、名前で追記する
- 特殊化するか、インライン化するか、どこで打ち切るかの判断も、鍵と呼び出し先のコードだけで決める。プログラム全体の大きさの予算は使わない。全体の予算を使うと、同じ名前の関数のコードがプログラムの残りに依って変わるためである (私の判断)
- 打ち切りの深さは、鍵そのものの入れ子の深さで決める。経路によって同じ鍵の複製のコードが変わらないようにするためである。打ち切りは引数の位置ごとに判断する。閉包の関数の深さに1を足した値が上限 (`MAX_KEY_DEPTH`、今は4) を超える位置だけを特殊化しない。こうすると、ある位置で閉包を開くかどうかが、その閉包と使う所だけで決まる。`unr` でない閉包は、使う所がすべて書き換わるときだけ開くので、この判断が呼び出しのほかの引数に依らないことが要る (実装の判断)
- 複製を作るときとインライン化するときに読む呼び出し先のコードは、translate の出力である。作った複製は、そのあと同じ規則で書き換える。作業リストの処理の順で、ほかの関数をどこまで書き換えたかが変わっても、複製のコードが変わらないようにするためである。`yields` の印のような解析の結果は、名前ごとに固定したコードの呼び出しの関係の上の最小の固定点で決めるので、順に依らない (私の判断)
- 関数の ABI は名前で決まる。使い方からは推し量らない (ユーザーが決めた。S13e-3 の作業のレビューで、使い方で決めると REPL の「同じ名前なら同じコード」が崩れると分かったため)。値として生まれる関数は内部の関数 (`internal`) で、ABI はいつも一様である。box の挿入がその場で一様にし、直接呼ぶ所では呼ぶ側が `box` と `unbox` を通す。ほかの関数は形を変えず、値として使う所には `f$boxed` を作る (`docs/spec/lir.md` の「一様な関数」)。複製は呼び出し先の印を写す。値として生まれた関数 (ラムダ、handle の本体、節と `return` の節) の複製は内部の関数で、ほかの関数の複製は内部の関数でない。インライン化と既知の閉包への `apply` の書き換えは、ある関数を直接呼ぶ定義と値として使う定義をプログラムごとに変えるが、ABI は変わらない
- S13e-2 の継続の関数 `f{H}$hN` は、`k` として節の写しに渡すものだけを `internal` にする。末尾で呼ぶだけの継続の関数、合流のブロックの関数 `f{H}$hjJ`、`k` を閉包で受ける節の写し `<g>{k}`、handler の複製 `f{H}` は、直接呼ばれるために生まれるので `internal` にしない。継続の関数をどちらにするかは、その `perform` の操作の節の形で決まり、節の形は名前で決まる
- `handle` を含む関数を複製するときは、その `handle` が指す節と `return` の節の関数も、複製の名前を持ち主として鍵に入れて一緒に複製する (`<callee>@{…} via <owner>`)。捕獲に既知の閉包がなくても複製する。EP は、節の関数がちょうど1つの `handle` に属することを前提にして、節の関数をその場で入口の関数に書き換えるためである (`crates/eml_core_ir/src/evidence/handlers.rs` の `entries`)。同じ鍵の複製は1つしか作らないので、複製した節の関数も1つの `handle` に属する。`handle` の本体の関数は、ほかの複製と同じ鍵で複製する。EP が1つの `handle` への所属を求めるのは節だけなので、本体の複製はいくつもの `handle` で共有してよい
- 名前の形は、instance の名前の後ろに引数の位置と閉包の関数を書く形にする (例: `"Std.List.map@[Int, Int]{0=main$lambda0}"`)。捕獲のある閉包は、関数の後ろに捕獲の数を書く (例: `{0=add3/2}`)。同じ関数の部分適用でも、捕獲の数が違えば複製の引数が違うので、数を書かないと同じ名前の関数が2つできる。64字を超えたら、今の instance の名前と同じくハッシュの形 (`f@#<16 hex>`) にする。ハッシュは、捕獲の数を含む名前全体からとる

### パスの後に直すもの

- `yields` の印: 書き換えた後の呼び出しから付け直す。純粋なラムダで特殊化した `map<e>` の複製は、中に `yields` の呼び出しが残らなければ `yields` でなくなる。プログラム全体の固定点で決める
- `internal` の印: 複製は呼び出し先の印を写す。S13e-2 の継続の関数 `f{H}$hN` は、`k` として渡すものだけに付ける (上の「鍵と名前」)。もとの関数の印は変えない
- 使われなくなった関数は消す。値として使われなくなった関数は、box の挿入が一様にしなくなるので、`f$boxed` と `box` / `unbox` も消える
- debug ビルドでは、`optimize` の後に Core IR の変換の段の verifier (`verify_translated`) をかけ、誤りを `invalid Core IR after optimize` と報告する。translate の線形性の検査 (`check_linearity`、`check_captures`) は translate の出力の上だけで動かす

### Kind の印

複製に足す引数と変数の印は、もとの変数の印を写す。閉包の捕獲を引数に開くときは、閉包の関数の捕獲の位置の引数の印と、呼び出し先のもとの引数の印のうち、小さい方 (`unr` < `poly` < `lin`) を使う。開いた引数の印がもとの引数の印を超えなければ、その引数を捕まえて作り直す閉包も、もとの引数を捕まえていた閉包も、組み立ての検査を通る。たとえば `Yield.for_each` は生産者を `poly` で受けるので、`File` を捕まえた生産者を開いた引数も `poly` になる。`Lin` の閉包 (`once`) の捕獲は、もとの閉包が1回しか使われないので、開いた引数も1回しか使われない。印の互換 (V2) と組み立ての検査は、今の verifier がそのまま確かめる。

## S13e-1 既知の閉包への特殊化

### 既知の閉包

次の変数を、既知の閉包とする。

- 同じ関数の中で `closure f(c_1, .., c_n)` か関数の値 `&f` で束縛された変数
- 特殊化した複製の中で、閉包の捕獲に開いた引数の組 (下の「書き換え」)

ブロック引数を通って合流する値と、データに入れてから取り出した値は、S13e-1 では既知としない。データを通る値は S13e-3 で扱う。

### 書き換え

- 既知の閉包への `apply` は、`call f(c_1, .., c_n, 引数…)` にする。引数の数が足りないときと余るときは、translate が呼ぶ相手の分かっている呼び出しにしている場合分け (`docs/spec/core-ir.md` の「変換の規則」の eval/apply) と同じに扱う
- translate が作る共有の包み (`op$`、`con$`、`cont$`、`cont$state`) は、複製の呼び出し先にせず、`call` にもしない。鍵の閉包の関数にはしてよい。複製の中で、引数のそろった `apply` が包んでいる命令に戻るためである。`docs/spec/core-ir.md` の「REPL の入力」のとおり、これらは直接呼ばれないことで、box の挿入がいつもその場で一様にするためである (`crates/eml_core_ir/tests/boxing.rs` の `shared_wrappers_are_never_called_directly_and_become_uniform_in_place` が固定する。確かめる内容は変えず、包む関数の値をデータを通して使う所に届けるようにプログラムだけを書き直す。下の「種類2」)。代わりに、引数のそろった `apply` を、包んでいる命令に戻す
  - `op$E.op` への `apply` は `perform E.op(引数…)` にする。`mask` の付いた `apply` は、`perform` が `mask` を持てないので書き換えない。`List.each yield lines` のような形が S13e-2 の経路の条件を満たすようにするためである (私の判断)
  - `con$C` への `apply` は `con C(引数…)` にする
  - `cont$` と `cont$state` への `apply` は、捕獲の生の継続への `resume` にする
- 既知の閉包を関数 g の引数に渡す直接の呼び出しは、特殊化した複製 `g{i=f}` の呼び出しにする。複製では、i 番目の引数を、捕獲の値の引数 n 個に置き換える
- 既知の閉包を、ほかの閉包 `closure h(.., p, ..)` の捕獲に入れるときも、同じく `closure h{i=f}(.., c_1, .., c_n, ..)` にする。`Iter.map f it` が作る閉包の中身や、`Iter.generate` の handle の本体に特殊化を届けるためである
- 複製の中の再帰 (`map f rest` のように、受け取った閉包をそのまま渡す呼び出し) は同じ鍵になるので、同じ複製を呼ぶ
- 複製の中で、閉包そのものが値として要る位置 (データに入れる、`return` する、相手の分からない `apply` の引数にする、特殊化しない呼び出しに渡す) では、その場で `closure f(c_1, .., c_n)` を作り直す

`handle` の本体、節、`return` の節も関数の値の位置である。`handle` の本体が既知の閉包なら、その閉包を作った式ごと特殊化の対象にする。S13e-2 が本体のコードを読めるようにするためである。

### 鍵

鍵は (呼び出し先の instance、引数の位置、閉包の関数、捕獲の Repr と印) である。捕獲の値そのものは鍵に入れず、引数として渡す。捕獲の Repr と印は、捕獲した値からでなく、閉包の関数の捕獲の位置の引数からとる。そのため捕獲の形は閉包の関数と捕獲の数で決まり、名前 (関数と数を書く) が鍵を区別する。値の Repr と引数の Repr の違いは、呼び出しの所で box の挿入が1回だけ直す (実装の判断)。

既知の閉包への `apply` でも、`unr` でない引数を、箱を要するスカラー (`int`、`enum`) の引数で受ける閉包の関数には、直接の呼び出しにしない。多相再帰の一様な instance の複製では、閉包の関数の引数の Repr が分かっても、値は型変数の印 (`poly`) のままである。box の挿入はこの値を `unbox` で読むだけなので、Perceus は値かその持ち主を `decref` で手放すことになり、`unr` でない値には出せない (`docs/spec/lir.md` の「Kind の印の検査」の V1)。この `apply` は残し、閉包は値として作り直す。一様な instance の中で既知の閉包が速くならないことは、S13e-1 の制限である

特殊化で生まれた閉包をまた鍵にする連鎖 (CPS 風に閉包が育つ形) は、鍵の入れ子の深さで打ち切る。打ち切った位置では閉包を作り直して、特殊化しない呼び出しにする。上限は「鍵と名前」の `MAX_KEY_DEPTH` である。

## S13e-2 handler への特殊化

### 対象

本体が既知の関数になった `handle E(init, body) { clauses } return ret` を対象にする。S13e-1 の後では、`Iter.generate` と `Yield.fold` の handle の本体は既知になる。

handle の本体から、末尾の位置の既知の直接の呼び出しだけでたどれる関数を、経路の関数と呼ぶ。末尾の位置は、EP と box の挿入と縮約が使うのと同じ定義 (`crates/eml_core_ir/src/tail.rs` の `tail_position_call`。`let x = call …` と `return x` の間に純粋な `let` だけがある形) で決める。経路の関数に次のどれかがあれば、その handle は特殊化せず、今の道に残す。

- `yields` の印の立った、末尾でない直接の呼び出しや `apply`
- 末尾の位置の、相手の分からない `yields` の `apply`
- `mask` の付いた呼び出し
- 入れ子の `handle`
- `resume`

Core IR は row を持たないが、yield しうる操作を持つエフェクトの `perform` は、それを含む関数の `yields` の印に必ず表れる (`docs/spec/types.md` の「row の具体化の記録」)。そのため、末尾でない呼び出しの先で H に届く `perform` が起きうるかは、この印で安全側に判断できる。`never` の操作だけのエフェクト (`Fail`) と `IO` の handle は、中断がすでに巻き戻しで安いので対象にしない。

ただし、`yields` の付け直しは再開する操作の `perform` だけを yield の地点に数えるので、`never` の操作だけを起こす関数は、付け直しで印を失う。そのため、扱うエフェクトが `never` の操作を持つときは、経路の関数の中の、`yields` でない関数への直接の呼び出しを translate の印でも見る。translate の印が立っていれば、その handle は特殊化しない。translate の印は関数の row から決まり、再開する操作を持つエフェクトのラベルは yield しうるので、その関数の中の `perform` を見落とさない。印は名前ごとに決まるので、判断も名前ごとに決まる (実装の判断)。

handle の地点と `perform` にも、次の条件を置く (実装の判断)。

- 本体、節、`return` の節は、関数の値か、handle を含む関数の中の `closure` の変数で、その変数を使うのはこの handle だけである。捕獲を複製の引数に開くので、閉包をほかの所でも使うと、`unr` でない捕獲を2か所で消費することになるためである
- 経路の関数の H 宛ての `perform` の引数は、節の関数の引数と同じ Repr を持ち、印も受け渡せる。節の本体は引数を置き換えて展開するので、Repr が違うと、box の挿入が直す呼び出しの境目がないまま、違う Repr の値を使うことになるためである。展開するすぐに再開する節では、引数の印が節の引数の印を超えないことも求める (`unr` < `poly` < `lin`)。`unr` の引数の所に `poly` の値を置くと、展開した本体がその値を何度も使い、`unr` でない値を写すことになる (`docs/spec/lir.md` の「Kind の印の検査」の V1)
- 状態つきの handler では、節の状態の引数の Repr と印が、`return` の節の状態の引数 (複製の状態の引数) と同じである。複製は受けた状態をそのまま節に渡すためである
- それ以外の節の `ret` の Repr は `return` の節の `ret` と同じで、印も受け渡せる。節の結果が、そのまま複製の結果になるためである
- 再開する操作のそれ以外の節では、`k` の出現が、`resume k(..)` の呼ばれる値、`k` を包む `closure cont$(k)` か `closure cont$state(k)` (handler の状態の有無と合う方)、`drop k` だけである。生の継続を入れ子の関数が捕獲する形は、その関数の中の `resume` も書き換えることになるので扱わない。`from ()` の状態つきの handler は、状態のない handler と同じに扱う (`init` が `()` なので見分けない)。状態の値が `()` なので、渡しても省いても同じである

### 節の形

節ごとに、Core IR の形から次のどちらかに分ける。`fn` と `ctl` の書き方ではなく、形で決める。

- **すぐに再開する節**: 節の関数 g の `k` の使いが、末尾の位置の `mask` のない `resume k(v, s)` だけである節。EP の「すぐに再開する節」(`docs/spec/core-ir.md` の「`perform` と節の3つの形」) の最初の3つの条件と同じである。4つ目の条件 (本体に yield の地点がない) は要らない。特殊化した節は handler の外側でふつうの関数として動き、本体の yield は EP が今までどおり扱うためである
- **それ以外の節**: `k` を値として使う節 (`Iter k`)、`k` を末尾でなく呼ぶ節、`k` を使わない節、`never` の操作の節。本体に `handle` を持つ節もここに入れる。展開した所ごとにその `handle` が写り、節の関数がちょうど1つの `handle` に属するという EP の前提 (`crates/eml_core_ir/src/evidence/handlers.rs` の `entries`) に反するためである (実装の判断)。写し `<g>{k}` は1つだけで、もとの g は使われなくなって消えるので、中の `handle` の節の関数も1つの `handle` に属する。写しは書き換える前の g から作るので、中の `handle` は特殊化しない

### 書き換え

経路の関数 f を、handler ごとの複製 `f{H}` にする。鍵は (f、handle の `return` の節の関数) で、名前は `<f>{H=<ret>}` である。translate は handle ごとに `return` の節を作り、S13e-1 は持ち主の名前を鍵に入れて節を複製するので、`return` の節の関数が handle を1つに決める。節の関数と捕獲の形も、それで決まる。64字を超えた名前は、S13e-1 の複製と同じく、f の名前の `@` より前の部分に名前全体のハッシュを付けた形にする (計画のときに決めた)。複製の引数は、f の引数、節と `return` の節の捕獲 (節の順)、状態つきの handler なら状態の順にする。状態のない handler の複製は、状態の引数を持たない。複製は `internal` にしない。

- すぐに再開する節への `perform E.op(a…)`: `perform` より後ろの部分を継続の関数 `f{H}$hN` に切り出し、節の関数 g の本体を `perform` の地点にその場で展開する。g の `return` のブロックの末尾の `resume k(v, s)` は、`f{H}$hN(生きている変数…, v, s)` の末尾呼び出しにする。状態のない handler では s を省く。状態がいつも関数の引数なので、ブロックの間で状態を持ち回る必要がなく、組も確保しない。費用は `perform` ごとの末尾呼び出し1回である。`perform` の後ろへジャンプする形でなくこの形にすることは、計画のときに決めた。g を呼んで `(v, s)` の組を返させる形にしないのは、状態つきの handler で `perform` のたびに組を確保することになるためである。g は再帰しないので、展開は止まる (私の判断)。展開した節の本体の文は、H 宛てとして書き換えない。節の中の `perform` は、もとの意味でも handler の外側で動くためである
- それ以外の節への `perform E.op(a…)`: `perform` より後ろの部分を継続の関数 `f{H}$hN` に切り出す。`f{H}$hN` は生きている変数を捕獲し、状態のない handler なら再開の値 v を、状態つきの handler なら v と次の状態 s を受けて、`f{H}` の残りを動かす。引数の形は、translate が節の入口で生の継続を包む `cont$` (`v` を受ける) と `cont$state` (`v` と `s` を受ける) と同じにする。節の関数の写しに `k` として `closure f{H}$hN(…)` を渡し、末尾で呼ぶ。写しの中では、`k` を包む `closure cont$(k)` と `closure cont$state(k)` を `k` そのものに替え、`resume k(v, s)` を `apply k(v)` か `apply k(v, s)` に替える。節の結果が、そのまま handle の結果になる
  - 写しの名前は `<g>{k}` で、鍵は節の関数 g だけである。g はちょうど1つの handle に属するので、状態の有無も g で決まる。64字を超えた名前は、ほかの複製と同じくハッシュの形にする。`k` の引数は位置を変えず、Repr を `tobj` にして印を保つ。`apply` の `yields` の印は g の印にする。EP が `resume` から作る `apply` と同じである (`docs/spec/core-ir.md` の「`apply` の印と分け方」)。`mask` はそのまま残す
  - 閉包の印は、g の `k` の引数の印にする。`once` の操作なら `lin` である。捕獲がなければ、関数の値 `&f{H}$hN` を渡す
- `never` の操作への `perform E.op(a…)`: 節の関数 g を、節の捕獲、a…、今の状態で末尾で呼び、`perform` より後ろは捨てる。g は `k` を受けないので写さない。もとの意味でも、中断は handle の地点まで巻き戻して節を動かすので、節の結果が handle の結果になる
- 複製を `perform` の地点で終える2つの形 (それ以外の節と `never` の操作) では、末尾の呼び出しにも閉包にも渡さない `unr` でない値を、呼び出しの前に `drop` する。対象は、節と `return` の節の捕獲と、`perform` の後で生きていた変数である。もとの意味でも、中断の巻き戻しと `prompt$` の `drop ret` が、そこで同じ値を捨てる。`unr` の値は Perceus が手放すが、`unr` でない値には `decref` を出せない (`docs/spec/lir.md` の「Kind の印の検査」の V1)
- 経路の関数の `return r`: `return` の節を、その捕獲、r、今の状態で末尾で呼ぶ。経路の関数でない関数への末尾呼び出し (`yields` でない関数の呼び出しと、`yields` でない `apply`) は末尾でない呼び出しにして、その結果で `return` の節を末尾で呼ぶ
- 経路の関数への末尾呼び出し: 引数に節と `return` の節の捕獲と今の状態を足して、`g{H}` を呼ぶ
- handle の地点: evidence を作らず、`body{H}(本体の捕獲…, (), 節と return の節の捕獲…, init)` を呼ぶ。状態のない handler では init を省く。handle のためだけに作った `closure` は消す

継続の関数の名前は `<f{H}>$h<N>` で、N は f の中の H 宛ての `perform` の、ブロックと文の順の通し番号である (0 から振る)。後ろの `perform` から書き換えるので、継続の関数の中の `perform` も同じ番号の継続の関数を呼ぶ。接尾辞を EP の継続の関数の `$kN` と分けるのは、`f{H}` がほかのエフェクトで yield すると、EP が `f{H}$kN` を作って名前が重なるためである (実装の判断)。継続の関数は、同じ handler の複製の中からだけ、直接呼ばれるか値として使われる。どちらになるかは、その `perform` の操作の節の形で決まり、節の形は名前で決まる。`k` として渡すものだけを `internal` にする (上の「鍵と名前」)。変数の表は、EP の継続の関数と同じく、本体に現れる変数だけに詰める (`docs/spec/core-ir.md` の「継続の関数」)。関数全体の表を地点ごとに写すと、地点の数と変数の数の積の大きさになるためである。

継続の関数の本体は、f の合流のブロック J (入る辺が2本以上か、引数を持つブロック) の手前で止める。J への `jump` は、合流のブロックの関数 `<f{H}>$hj<J>` を、`jump` の実引数、J の入口で生きているほかの変数 (状態を除き、番号の昇順)、状態つきなら今の状態の順で末尾で呼ぶ形にする。`f{H}$hjJ` の本体は J とそこから届くブロックで、次の合流のブロックで止まり、中の `perform` も同じ規則で書き換えてある。`internal` にせず、名前は継続の関数と同じく `limited` の形に接尾辞を外から付ける。EP の継続の関数が合流のブロックで `f$jJ` を呼ぶのと同じ形で、コードの大きさを `perform` の数に比例させるためである。合流のブロックを写すと、条件つきの `yield` が K 個並ぶ生産者で、合流のブロックとその先がその前のどの継続の関数にも入り、K = 250 で Core IR が 28万行になった (最終の見直しで見つけた。合流のブロックの関数にした後は 6千行)。展開した節のブロックは合流のブロックにしない。1つの `perform` の地点からだけ届くので1つの続きにしか入らず、節が状態を使った後なので、状態を渡す形にも合わないためである (実装の判断)。接尾辞を EP の `$jJ` と分けるのは、`$hN` と同じ理由である (実装の判断)。生存は複製のブロックを後ろから1回だけたどって求め、`perform` はたどる途中で書き換える。`perform` ごとに続きをたどり直すと、`perform` の数の2乗の手間になるためである。

`f{H}$hN` は `once` の継続なら `Lin`、`multi` の継続なら捕獲がすべて `Unr` になる。持ち越し規則 (`docs/spec/linearity.md`) が、`multi` の継続の捕獲を `Unr` に限っているためである。

### 意味が変わらない理由

- 節の中の `perform` は、もとの意味でも handler の外側で動く。特殊化した後は `evv` に H がないので、外側の handler に届く
- 経路の関数の中の、ほかのエフェクトの `perform` も、外側の handler に届く。H は E だけを扱い、経路に入れ子の handle も `mask` もないためである
- 経路の関数の中で外側のエフェクト (`State` など) が yield するときは、EP が今までどおり継続を作る。その継続には `f{H}` の残りが入るので、H の handle のフレームを含む今の継続と同じ意味になる
- `File` を持つ生産者では、継続 `closure f{H}$hN` が `File` を捕獲する。継続を捨てると `File` も閉じる。今の `Iter.generate` の注釈 (継続を捨てるときに閉じる) と同じである

### `Iter` と push の形での結果 (見込み)

- `Iter.generate (nums n)`: `go{H}` の `yield i` は、`tail call yield節'(i, closure go{H}$h0(n, i))` になる。節は `Some (i, Iter k)` を作って返す。`go{H}` は自分を呼ばなくなり、再帰は継続 `go{H}$h0` の中に移る。1要素あたりの yield は0回、確保は4回 (`k`、`Iter`、組、`Some`) になる。節が `k` を値として包む `closure cont$(k)` は、上の書き換えで `k` そのものになるので、閉包は増えない
- `Yield.fold f init (nums n)`: `go{H}(n, i, acc)` の `yield i` は、`f acc i` を呼んで新しい状態を作り、`go{H}(n, i + 1, acc')` を末尾で呼ぶ形になる。`f` が既知なら、S13e-1 で直接の呼び出しになる

## S13e-3 インライン化と融合

### インライン化

- 対象は、再帰しない小さな関数と、特殊化で生まれた複製である。大きさのしきい値は、計測しながら作る人が決める。判断は呼び出し先のコードだけで決め、名前が同じ関数のコードを変えない (上の「鍵と名前」)
- 関数は、直接の呼び出しの関係で呼ばれる関数から先に、1回ずつ書き換える。展開する本体は、呼び出し先の書き換えた後のコードから写し、その中の呼び出しは呼ぶ側で展開し直さない。展開してよいかは、呼び出し先の書き換えた後の大きさで決める。こうすると、関数のコードはその関数と呼び出し先のコードだけで決まり、展開を重ねても大きさは呼び出しの連なりの深さに比例する (S13e-3 の作業のレビューで決めた。元のコードから写して呼ぶ側ごとに展開し直すと、展開の深さの打ち切りがプログラムによって違う所で効き、コードが深さの指数で育った)
- 再帰の SCC の中の関数はインライン化しない。特殊化だけで扱う
- インライン化した関数の extern の呼び出しは、もとの位置を持ったままにする。実行時エラーの位置 (`at path:line:column`) を変えないためである
- Prelude の `|>`、`>>`、`<|` のような合成子も、この規則で展開する (ロードマップの「jump threading とインライン化」)

### 既知のコンストラクタ

- `con` で作った値への `switch` と `unpack` を、その場で解く。ブロック引数を通して届く場合も解く (jump threading)
- 呼び出しの結果に対する case-of-case と、translate の後に現れる case-of-case も、インライン化の後でここで扱う (ロードマップの「jump threading とインライン化」)

### `con` の値の形についての特殊化

S13e-1 の「既知」を、既知の閉包をフィールドに持つ `con` の値にも広げる。鍵は (呼び出し先、引数の位置、コンストラクタ、フィールドごとの閉包の関数か Repr と印) である。捕獲の値とスカラーのフィールドは鍵に入れず、引数にする。

`Iter.fold f 0 (Iter.generate (nums n))` では、次のように融合する (見込み)。

1. `generate{nums}` をインライン化すると、`fold` の引数が `Iter (closure lambda0{nums}(n))` という既知の形になる
2. `fold` をその形について特殊化し、`next` と、S13e-2 の後で再帰しなくなった `go{H}` をインライン化する。`Some (i, Iter (closure go{H}$h0(n, i)))` がその場で `match` され、既知のコンストラクタとして解ける
3. 再帰の呼び出しの引数の形は `Iter (closure go{H}$h0(…))` で、要素ごとに同じなので、2つ目の複製に収まる。結果は `fold''(f, acc + i, n, i + 1)` のループで、1要素ごとの確保はない

### 止まること

S13e-1 と同じく、鍵の入れ子の深さで打ち切る。インライン化は、呼ばれる関数から先に1回ずつ書き換え、展開した本体を展開し直さないので、深さの上限を置かなくても止まる (上の「インライン化」)。

## S13e-4 reuse analysis

`eml_rt` には、reuse の部品 (`release_reuse`、`con_at`、`reuse_in_place`、`drop_token`) と検査モードの扱いが S9 で入っている (`docs/spec/object-model.md`)。S13e-4 で作るのは、LIR のパスと VM の命令と verifier の規則である。

### 規則

- Perceus の後に、LIR のパスとして置く。Perceus が出した `release` を `release_reuse` に、その後ろで同じ物理的な語の数の箱を作る `con` を `con@token` に書き換える (Lorenzen と Leijen の drop-guided reuse の向き)。組は、物理的な語の数と位置で決める (`docs/spec/object-model.md` の「物理的な形」)
- 同じコンストラクタで作り直す場合 (`{ st | total = … }` のレコードの更新や、`upsert` の `Node` の作り直し) は `reuse_in_place` にし、変わったフィールドだけを書く。`lin` ビットの条件 (古い物体の印が `unr` か、新しい物体の印が `lin` のときだけ `reuse_in_place`。ほかは `con_at`) を守る
- 共有の印の付いた物体は reuse しない。`release_reuse` が実行時に空の token を返し、`con@token` は新しく確保する
- token は関数の外に出さない。呼び出しの引数、閉包の捕獲、`return` には渡さない。そうすれば、EP の後の継続のクロージャに token が入ることはない (ロードマップの「Perceus の最適化」の約束)
- token を yield の地点をまたいで生かさない。yield しない呼び出しはまたいでよい。std の `map` は `go (f x :: acc) rest` の形で、`ys` の節を手放してから `f x` を呼ぶので、呼び出しをまたげないと最も多い形で reuse が効かないためである (私の判断)
- 中断の巻き戻しでは、生きている token を `drop_token` で手放す。`eml_lir::unwind` が作る巻き戻しの表に token の slot を足す
- 使わずに終わる道の token は、その道で `drop_token` する

### `RunStats` に足す欄

S13e-1 と S13e-3 の確かめのため、S13e-1 で確保の回数 `allocations` を足す (`docs/implementation/benchmarks.md` の欄の一覧も直す)。S13e-4 では、reuse の回数 `reuses` を足す。どちらも検査モードでだけ数える。欄の名前は作る人に任せる。

## 計測の記録

- `bench/` に足すプログラム: 上の「計測 (S13e の前)」の表の形 (注文の集計、push の `Yield.fold`、`Iter` を重ねた形、総称な木の `fold`)、`Yield` で書いた生産者を `Iter.generate` で引く形、比べるための直接のループ、`logstat` の `upsert` と `sort_by` の形。名前と要素の数は作る人に任せる。`crates/eml_vm/tests/bench.rs` が `RunStats` を固定する
- 手で特殊化した版は、S13e-1 の確かめのために `bench/` に置くか、テストの中の比較に使う。置き方は作る人に任せる
- 各作業の終わりに `bench/run.sh` を流し、`docs/implementation/benchmarks.md` に「S13e-1」から「S13e-4」の節を足して記録する。S13e-4 では `bench/scripts.sh` も流す
- 上の「計測 (S13e の前)」の表の値を、`benchmarks.md` の「S13e-1」の節に前の値として写す

## 文書の更新

- `docs/future/roadmap.md`: S13e の節を4つの作業に分け、「決めたこと」と「論点」の答えを書き、その後の項目から S13e に入れた部分を外すことは、この設計と一緒に済ませた。各作業の終わりに、その作業を「済み」にし、決めたことの行き先を書く
- `docs/spec/core-ir.md`: 「パス」の表に `optimize` を足し、新しい節「最適化」に、鍵と名前、既知の閉包、handler への特殊化、インライン化と融合の規則を書く。「表示関数」と「REPL の入力」の名前の規則に、特殊化した複製の名前を足す
- `docs/spec/lir.md`: 「パスの順番」に reuse を足し、`release_reuse`、`con@token`、token の変数の規則、verifier の検査、テキストの形を書く。「reuse analysis と借用パラメータの最適化は後で追加する」を直す
- `docs/spec/vm.md`: reuse の命令と、巻き戻しの表の token の slot を書く
- `docs/implementation/architecture.md` と `CLAUDE.md` (英語のまま): パイプラインに `optimize` と reuse を足す
- `docs/implementation/status.md`: 特殊化しない形 (末尾でない再帰の生産者、データを通る閉包のうち S13e-3 の形に当たらないもの) を制限として書く
- `docs/implementation/benchmarks.md`: 上の「計測の記録」

## テストの変更

### 種類1 (通過と失敗の変更)

今の時点で分かっているものはない。動作は変えないので、UI テストと REPL のトランスクリプトの出力は変わらない見込みである。

実装の途中で、通過と失敗が変わるテスト (VM の深さの上限に当たるテストで、インライン化でフレームが減って上限に当たらなくなるものなど) が見つかったら、作る人はコミットの前にこの節に1つずつ足し、理由を書いて、ユーザーの確認を受ける。

### 種類2 (期待値の変更)

次の範囲の期待値は、規則の変更に合わせて書き換えてよい。理由は、EP の前に `optimize` が入り、LIR の後に reuse が入ったことである。

- `crates/eml_core_ir/tests` の、`Pass::Evidence` の後の Core IR と LIR の段の出力のスナップショットのうち、特殊化、インライン化、`yields` の印、関数の名前と並びが変わるもの
- `crates/eml_lir/tests` と `crates/eml_vm/tests` の、Perceus の後の LIR と逆アセンブルのスナップショットのうち、reuse の命令が入るもの
- `crates/eml_vm/tests/bench.rs` と `crates/eml_vm/tests/scaling.rs` の `RunStats` のうち、回数が変わるものと、足した欄
- `crates/eml_cli/tests/api.rs` などで、関数の名前を含む内部の誤りの文言が変わるもの
- 包む関数や既知の閉包への、特殊化しない `apply` が残ることを前提にしていたテストのプログラム。テストが確かめる内容は変えずに書き直す。値はデータを通して使う所に届け、包む関数と同じ形のローカル関数には本体に計算を足す (`crates/eml_core_ir/tests/boxing.rs` の `shared_wrappers_are_never_called_directly_and_become_uniform_in_place`、`crates/eml_core_ir/tests/evidence.rs` の `clause_local_function_resumes_once`)。S13e-1 の複製の後は、高階関数に既知の閉包を渡して、特殊化しない `apply` や値として使う関数の形を見ていたテストも、閉包を `data Box a = | Box a` に入れて取り出してから渡す形に書き直す。書き直したのは `boxing.rs` の7つ (`a_lambda_used_only_as_a_value_is_made_uniform_in_place`、`a_top_level_function_used_only_as_a_value_gets_a_boxed_wrapper`、`a_function_also_called_directly_gets_a_boxed_wrapper`、`a_function_made_uniform_by_t3_gets_no_boxed_wrapper`、`a_value_unboxed_by_the_pass_is_passed_on_without_a_new_box`、`a_value_lambda_that_fails_ends_in_a_tail_never_clause`、`t3_raises_the_ret_along_a_chain_of_tail_calls`)、`contract.rs` の `calls_in_tail_position_are_tail_calls`、`evidence.rs` の4つ (`a_masked_call_restores_the_evidence_list_and_wraps_the_continuation`、`a_masked_call_in_a_continuation_restores_the_evidence_list_before_it_continues`、`a_pure_instance_of_a_row_polymorphic_function_has_no_yield_check`、`a_masked_call_in_a_non_yielding_function_has_no_slow_path`)、`translate.rs` の `a_boxed_wrapper_is_named_after_its_instance`、`local_functions.rs` の `std_public_functions_keep_their_lir` (の `STD_PROGRAM`) である。`boxing.rs` の `main` と `via` の期待値は、`hide` の呼び出しと `unpack` が増えた分だけ変わる
- S13e-2 の handler への特殊化で handle が消えるようになったテストのプログラム。EP が handle、`perform`、`resume` を下ろす形を見るテストは、handle の本体を `hide` でデータに通して渡す形に書き直し、確かめる内容は変えない。書き直したのは `evidence.rs` の13 (`a_handle_installs_evidence_and_calls_prompt`、`an_immediate_clause_becomes_its_entry`、`a_clause_that_performs_another_effect_is_not_run_in_place`、`a_perform_looks_up_evidence_and_calls_its_clause`、`closures_ep_builds_take_the_join_of_their_captures`、`an_io_extern_is_not_a_yield_point`、`a_never_clause_call_is_not_a_yield_point`、`continuations_of_many_yield_points_stay_linear_in_size`、`a_fn_clause_runs_in_place`、`a_fn_clause_that_performs_yields`、`a_fn_clause_that_prints_runs_in_place_under_the_outer_evv`、`a_fn_clause_that_calls_nothing_has_no_under`、`a_fn_clause_that_fails_runs_in_place`)、`translate.rs` の `a_lin_enum_variable_is_unr`、`crates/eml_vm/tests/scaling.rs` の `every_operation_looks_up_its_evidence` である。`a_perform_looks_up_evidence_and_calls_its_clause` は、`perform` を下ろした関数が本体のラムダ `main$lambda0` になるので、その関数の表示を見る。`a_handle_installs_evidence_and_calls_prompt` の `main` の期待値は、`hide` の呼び出しと `unpack` が増えた分だけ変わる。`boxing.rs` の `a_tail_call_off_any_loop_may_stay_an_ordinary_call` は、handle の本体の形 (`answer` の結果の `box`) を見るので、本体でなく `answer` の中で `ask` をデータに通す。S13e-1 の規則だけを見る `optimize.rs` の4つ (`yield_as_a_value_becomes_a_perform`、`each_yield_is_specialized`、`two_handler_copies_keep_their_own_clauses`、`a_clause_copy_in_a_translated_function_is_internal`) も、生産者をデータに通して渡す。それ以外の節の特殊化 (S13e-2 の Task 3) の後は、`evidence.rs` の4つ (`a_clause_that_does_not_resume_at_once_yields_to_its_handler`、`a_resume_becomes_an_apply`、`a_resume_in_a_yielding_function_becomes_a_yielding_apply`、`closures_ep_builds_take_the_join_of_their_captures`) も、handle の本体 (最後のものは `put`) をデータに通して渡す。期待値は変わらない。`optimize.rs` の `generate_reaches_the_handle_body` は、S13e-1 の既知の閉包が handle の本体に届くことを見るので、handle を残すために、生産者が `yield` を末尾でない呼び出し (`emit`) で起こす形にする
- 関数の名前の一覧と、複製を含む Core IR の期待値。`crates/eml_core_ir/tests/instances.rs` の `a_closure_with_a_file_passed_as_a_type_variable_lowers` は、`f` の2つの instance の代わりに、既知のラムダごとの複製の名前を並べる。`evidence.rs` の `an_extern_wrapper_does_not_yield` は、`$extern` を名前に含む複製 (`yields` でない) も並べる
- S13e-3 のインライン化で、調べる関数や呼び出しが呼ぶ側に展開されるようになったテストのプログラム。確かめる内容は変えずに書き直す。後のパス (EP、box の挿入、縮約、Perceus) の形を見るテストは、調べる関数をデータに通して呼ぶ (`let Box f = hide 0 f`)。`hide` は自分を呼ぶ形 (`hide n x = if n <= 0 then Box x else hide (n - 1) x`) にして、展開されないようにする。展開される `hide x = Box x` では、後の既知のコンストラクタの規則が値を見通すためである。前の項目までで `hide` を使うテストも、すべてこの形にする。取り出した関数の値への `apply` は直接の呼び出しでないので、関数は自分のコードのまま残る。調べる関数の中で直接の呼び出しのまま残す必要がある呼び出し先と、呼び出しの安全点を残す必要がある関数は、自分を呼ぶ形にして展開されないようにする。書き直したのは `contract.rs` の4つ (`a_returned_if_value_becomes_tail_calls_in_each_arm`、`a_returned_match_value_becomes_tail_calls_in_each_arm`、`returning_a_field_of_a_call_result_is_not_a_tail_call`、`calls_in_tail_position_are_tail_calls`)、`boxing.rs` の9つ (`a_function_also_called_directly_gets_a_boxed_wrapper`、`fields_are_rebound_at_the_head_of_the_case_target_and_after_an_unpack`、`int_constants_are_boxed_and_other_constants_pass_as_they_are`、`a_value_unboxed_by_the_pass_is_passed_on_without_a_new_box`、`a_unit_value_passes_to_tobj_without_an_instruction`、`a_function_that_fails_with_a_never_operation_keeps_its_scalar_ret`、`the_result_of_exit_is_not_compared_with_positions`、`t3_raises_the_ret_along_a_chain_of_tail_calls`、`a_tail_call_off_any_loop_may_stay_an_ordinary_call`)、`evidence.rs` の4つ (`a_masked_call_in_a_continuation_restores_the_evidence_list_before_it_continues`、`a_pure_instance_of_a_row_polymorphic_function_has_no_yield_check`、`evidence_and_evv_are_unr_and_state_is_poly`、`an_extern_wrapper_does_not_yield`)、`local_functions.rs` の2つ (`a_lifted_function_has_the_same_lir_as_a_top_level_helper` の `PAIRS`、`std_public_functions_keep_their_lir` の `STD_PROGRAM`。値として使う std の関数に box の挿入が足す `$boxed` は比べない)、`perceus.rs` の11 (`strings_are_dupped_and_decreffed`、`equations_allocate_no_tuple_for_their_arguments`、`a_default_target_owns_the_scrutinee_without_a_dup`、`a_dead_scrutinee_whose_fields_are_unused_is_decreffed`、`a_non_tail_if_keeps_strings_used_later`、`a_nested_pattern_releases_from_the_parent_down`、`a_known_value_that_skips_its_field_releases_it`、`constructor_arguments_are_owned_by_the_value`、`a_string_switch_leaves_the_scrutinee_to_its_targets`、`interpolation_keeps_only_the_accumulator_live_across_holes`、`a_discarded_call_result_is_released`)、`translate.rs` の6つ (`a_constructor_bound_by_let_is_known_later`、`a_linear_constructor_variable_received_by_an_arm_is_taken_apart`、`a_linear_constructor_variable_is_opened_when_no_arm_receives_it`、`a_projection_of_a_type_variable_field_is_unboxed`、`a_linear_constructor_variable_used_on_another_path_is_not_opened`、`a_lin_enum_variable_is_unr`)、`optimize.rs` の3つ (`unreachable_functions_are_dropped`、`a_caller_without_yield_points_stops_yielding`、`closures_with_different_capture_counts_get_different_copies`)、`crates/eml_cli/tests/api.rs` の `a_file_left_open_by_an_interrupt_is_closed_by_recovery` (`keep` を自分を呼ぶ形にし、`File` を渡す呼び出しの安全点を残す) である。`interpolation_keeps_only_the_accumulator_live_across_holes` の呼び出しの数は、穴の `display` が展開されるので、穴ごとに2つから1つになる
- 関数の ABI を名前で決めるようにしたので、ローカル関数、EP の `f$jJ`、S13e-2 の `f{H}$hjJ` と `<g>{k}` と末尾で呼ぶだけの `f{H}$hN` の表示から `internal` が外れ、ラムダの複製に `internal` が付く。直接呼ぶラムダは一様な引数を受ける。`crates/eml_core_ir/tests/instances.rs` の `a_closure_with_a_file_passed_as_a_type_variable_lowers` の一覧から、内部の関数になった複製が外れる
- 小さな `main` は入口の関数に展開される。`main` や展開された関数の表示を見ていたテストは、入口の関数 (`entry$main`、REPL では `entry$input$N`) の表示を見る。展開と、使われない純粋な `let` の削除で、`optimize` の後の Core IR、LIR、関数の名前の一覧の期待値が変わる (`optimize.rs`、`instances.rs`、`session.rs`、`translate.rs` の `lower_until_stops_after_the_named_pass`、`crates/eml_test_support/tests/support.rs` の `lir_until_stops_after_the_named_pass` など)。`lower_until_stops_after_the_named_pass` の使われない `let` は、縮約より前に融合の段が消す
- 既知のコンストラクタの規則 (その場で解くことと jump threading) と、解いたフィールドの既知の閉包への `apply` の書き換えで、`optimize` の後の Core IR と LIR の期待値が変わる (`optimize.rs` の `a_closure_only_stored_in_data_is_not_specialized`、`local_functions.rs` の `std_public_functions_keep_their_lir`)。`crates/eml_vm/tests/bench.rs` の `RunStats` も変わる。`fold_state`、`iter_pipeline` とその手で書いた版、`upsert` とその手で書いた版の確保と RC の回数が減る。`upsert` の2つは `unboxes` が増える。`Some v` を解くと、型変数のフィールドから取り出した値を `Int` として使う所が増え、box の挿入が case の行き先の先頭で `unbox` するためである。調べる値が解けるようになったテストは、確かめる内容を変えずに書き直した。値を `hide` に通したのは、`optimize.rs` の `a_constructor_value_becomes_a_con` と `crates/eml_vm/tests/scaling.rs` の `an_int_in_a_type_variable_field_is_boxed_and_unboxed` である。`optimize.rs` の `an_over_saturated_known_apply_calls_then_applies_the_rest` は、閉包を返す関数を自分を呼ぶ形 (`adder`) にした

### 足すテスト

- `crates/eml_core_ir/tests` に、`optimize` の各規則 (既知の閉包への `apply`、引数と捕獲の特殊化、`op$` の `perform` への書き換え、鍵の共有と打ち切り、`yields` の付け直し、handler への特殊化の条件と2つの節の形、状態の受け渡し、インライン化、既知のコンストラクタ、形についての特殊化) を、ソースからの Core IR のスナップショットで確かめるテスト
- REPL のトランスクリプトに、前の入力の高階関数に後の入力のラムダを渡す形を足す。特殊化した複製が追記の「同じ名前なら同じコード」の確かめを通ることを固定するためである
- `crates/eml_lir/tests` に、reuse の手書きの LIR のテスト (同じ語の数、`reuse_in_place`、token が呼び出しと yield の地点をまたぐ規則、使わない道の `drop_token`) を足す。verifier が token の誤用を断ることも確かめる
- UI テスト (`tests/ui/run/`) に、`File` を持つ生産者を特殊化した `Iter` で引いて途中で捨てる形と、`multi` の継続を2回再開する handler を特殊化する形を足す。checked mode のリークの検査で、後始末が今と同じことを確かめる
- Miri: `eml_rt` に新しい FFI の境界は入らないので、足さない

## 作る人に任せること

- `optimize` のモジュールの分け方と、作業リストの形
- 特殊化した複製の名前の細部の書き方
- 打ち切りの深さとインライン化の大きさのしきい値。計測で決め、`benchmarks.md` に理由を書く
- 継続の関数 `f{H}$hN` の切り出しを、EP の継続の関数を作る仕組みと共有するか、別に作るか
- reuse の組の選び方 (1つの `release` に後ろの `con` が複数あるときにどれを選ぶか)
- `RunStats` に足す欄の名前
- 各作業の計画のタスクの分け方
