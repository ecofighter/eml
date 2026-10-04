# Core IR の join point の解析の整理と最適化の設計

位置づけ: 作業用の設計文書。作業を終えたら、残す価値のある内容を `docs/spec/core-ir.md`、`docs/implementation/architecture.md`、`docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

Core IR の join point について、2つのことをする。

1. 解析の整理。join point の本体が使う外側の変数を IR の欄にし、生存解析を関数ごとに1回にまとめる。verifier は生存解析に頼らずに検査する。
2. 生成コードの最適化。整理した IR の上に、join point を書き換えるパス `simplify` を置く。

2つは別の段階として順に入れる。段階Aは整理だけで、実行の振る舞いも生成するコードも変えない。段階Bで `simplify` を入れる。

### 今の問題

- `liveness` を関数ごとに4回計算している。Perceus で1回 (RC の対象)、verifier で2回 (RC の対象とすべての変数)、`saved.rs` で1回 (すべての変数) である。
- 式ごとに変数の集合を写して持つので、時間もメモリも「式の数 × 生きている変数の数」かかる。文の長い列では2乗になる。
- join point の本体が使う変数 (`needs`) は IR に書かれておらず、使う側がそのつど計算し直す。verifier も Perceus と同じ `liveness` を使うので、`liveness` の誤りを verifier が見逃す。

### 範囲の外

- 再帰する join point (ループ) と、局所関数を join point にする変換 (contification)。ただし、後で入れるときに解析と IR の形を作り直さずに済むようにする (下の「ループへの備え」)。
- 大きな本体を jump の位置に写すこと。コードが増え、判断の基準が要るためである。
- コンストラクタの値での case-of-case。段階4の `match` で、フィールドの束縛と割り当ての省略と一緒に設計する。
- インタプリタの変更。jump は今までどおり環境をそのまま使う。

### ユーザーと合意済みの決定

- 最適化の対象は、解析そのものの費用と生成コードの両方とする。解析を先に整理し、その上に生成コードの最適化を載せる。
- ループは今回入れず、形だけ備える。
- join point が受け取る外側の変数を IR の欄にする (案A)。IR を変えずに解析だけまとめる案 (案B) は、ループや evidence passing の段階で結局この形に移ることになるので採らない。生きている変数をすべて jump の引数で渡すブロック引数の形 (案C) は、jump の引数が生きている変数の数だけ増え、M1 には重すぎるので採らない。

## 段階A: 解析の整理

### A1. IR の形

`CExpr::Join` に `captures` を足す。

```rust
Join {
    join: JoinId,
    param: VarId,
    /// 本体が使う外側の変数 (`param` を除く)。RC の対象かどうかによらずすべて入れる。並びは `VarId` の昇順。
    captures: Vec<VarId>,
    body: CExprId,
    scope: CExprId,
}
```

- 表示は `join j3(t5) [x1, f2] {` とする。`captures` が空なら `[]` を書く。
- 引数は1つのままにする。ループを入れるときに `Vec` にする。
- 本体から自分へ jump しない制約は残す。

### A2. 解析

`liveness.rs` を作り直し、次の関数1つにする。

```rust
/// `captures` を埋め直し、ブロックの入口ごとの生きている変数を返す。
pub(crate) fn analyze(function: &mut CoreFn) -> BlockLiveness;
```

- 対象はすべての変数である。RC の対象だけの集合が要る側は、結果を RC の対象で絞る。生存は変数ごとに独立して決まるので、絞った結果は RC の対象だけで解析した結果と一致する。
- `BlockLiveness` は、ブロックの入口ごとに、そこで生きている変数を持つ。ブロックの入口は、`Switch` の各枝と、join point の範囲である。join point の本体の入口は `captures` と `param` で決まるので、表に持たない。
- `Let` ごとの集合は持たない。連鎖は、集めてから逆順にたどり、1つの集合を更新して入口の集合を求める。
- join point の本体を範囲より先に処理する。範囲の中の jump と、範囲の中の join point の本体からの jump が、行き先の `captures` を要るためである。今の `liveness` と同じ順である。
- jump の時点で生きている変数は、行き先の `captures` と渡す値である。
- `Let` の連鎖と join point の本体の連なりは長くなりうるので、今の `liveness` と同じく、再帰せずに作業の列で後順にたどる。
- 費用は「式の数 + ブロックの入口の集合の大きさの和」に比例する (集合の操作の対数の分を除く)。

### A3. パスの順と、解析を呼ぶ場所

`lower` の流れを、`lower` → Perceus (解析と `saved` を含む) → verifier にする。`saved.rs` は消す。

- Perceus は、関数ごとに最初に `analyze` を呼び、`captures` を埋めて表を受け取る。
- 段階Bでは、`lower` と Perceus の間に `simplify` を挟む。`simplify` は `captures` を使わないので、Perceus の前に欄が古くても困らない。
- 本体を書き換えるパスを今後足すときも、欄を使う前に `analyze` を呼び直す。

### A4. Perceus

所有の約束と `dup` / `decref` の置き場所は、今と同じにする。変えるのは、集合の求め方である。

- 前向きの走査では、連鎖の段を集めるだけで、`Let` ごとの集合を求めない。連鎖の2段目より後の所有は、「その時点で生きている RC の対象 + 直前の段で束縛した変数」と一致するためである。そのため、`Let` の前で捨てる変数は、直前の段で束縛した変数が死んでいる場合だけになる。連鎖の最初の段 (関数の入口、`Switch` の枝の入口、join point の本体の入口) では、入口の所有から、生きていない変数を捨てる。
- 連鎖の終わり (`Return`、`TailCall`、`Jump`、`Switch`) で生きている変数は、表と `captures` から求める。逆順に組み立てるときに、この集合を1段ずつ更新する。その集合で次の3つを決める。
  - 使った変数の `dup`。後でも使う変数なら、その分を複製する。
  - 死んだ変数の `decref`。
  - 呼び出しの `saved`。呼び出しの後で生きている変数、つまり RC の対象でない変数も含めた集合である。Perceus は、後で使わない RC の対象を呼び出しの前に decref するので、この集合は Perceus の後の生存とも一致する。
- join point の本体は、`captures` のうち RC の対象と、`param` (RC の対象なら) をちょうど1つずつ所有して始まる。jump の前では、それ以外を捨てる。渡す値を本体でも使うなら、複製する。今の `needs` を `captures` から求める形になる。

### A5. verifier

verifier は `liveness` を使わない。`captures` を、関数の引数と同じく宣言として扱う。

- `Join` を定義する時点で、`captures` の変数がすべて範囲にあることを確かめる。並びが昇順で重複がないことも確かめる。
- join point の本体の範囲は、`captures` と `param` と本体の中の束縛だけにする。`captures` の書き漏れは、本体での範囲の誤りとして報告される。
- jump の時点では、`captures` の変数が範囲にあることと、`captures` のうち RC の対象をちょうど1つずつ所有していることを確かめる。今の `uses` と `needs` の検査を、この2つで置き換える。
- `captures` が本当に使う変数より多くても、検査は通る。余分な変数は、本体で Perceus が decref するためである。最小であることは不変条件にせず、解析の性質として扱う。

### ループへの備え

- ループで値が変わる変数は引数で受け、変わらない変数は `captures` で受ける。
- jump の時点で生きている変数は「行き先の `captures` + 渡す値」と決まる。自分への jump もこの規則に従うので、本体の自由変数を数えるだけで `captures` が決まり、生存解析に不動点の計算は要らない。互いに jump し合う join point の組を入れる場合は、別に設計する。
- 自分への jump を許す日には、verifier の制約を外し、変換に contification を足せばよい。

## 段階B: `simplify`

### B1. 置き場所

- 新しいパス `simplify.rs` を、`lower` の後、Perceus の前に置く。RC の命令がまだないので、書き換えで所有権を扱わずに済む。
- デバッグビルドでは、Perceus の後の verifier が、最適化した IR も確かめる。
- 1回の実行では、B2、B5、B3、B4 の順に1巡だけ回す。B5 を B4 より先に回すのは、B5 で jump がなくなった join point を、同じ巡の B4 で消すためである。不動点まで回すかは、テストで2巡目が要る形が出たら決める。
- 書き換えは式のアリーナの上でその場で行い、木から外れた式はアリーナに残す。Perceus がアリーナを作り直すときに捨てる。

### B0. 変換: 末尾にない `if` の条件を範囲の中に置く

今の変換は、末尾にない `if` の条件の計算を、join point の定義より前に置く。条件がさらに末尾にない `if` (`&&` や `||` を含む) だと、外側の `if` の join point が、条件の join point の本体の中に入る。

```
join j1(t2) {                  -- 条件の join point
  join j0(t3) { 続き }          -- 外側の if の join point
  switch t2 { #0 -> jump j0(2), #1 -> jump j0(1) }
}
switch a0 { #0 -> jump j1(#0), #1 -> jump j1(b1) }
```

この形では、`j1` の本体が `Switch` でないので B2 が効かない。枝を切り出しても、枝が jump する `j0` が `j1` の本体の中にあり、範囲の外になる。

そこで、変換は `if` 全体 (条件の計算を含む) を join point の範囲として組み立てる。join point の定義は実行時に何もしないので、条件を計算する位置を範囲の中に移しても、評価の順は変わらない。

```
join j0(t3) { 続き }
join j1(t2) { switch t2 { #0 -> jump j0(2), #1 -> jump j0(1) } }
switch a0 { #0 -> jump j1(#0), #1 -> jump j1(b1) }
```

### B2. 分かっているタグの jump (case-of-case)

本体が引数を scrutinee にした `Switch` だけでできている join point に、定数のタグを jump で渡している場合に書き換える。`&&` と `||` は `if` に脱糖されているので、`if a && b then x else y` がこの形になる。

- 本体の `Switch` の各枝を、それぞれ新しい join point に切り出す。新しい join point は値を受けないので、引数は `()` を受ける新しい変数にする。ループで引数を `Vec` にするときに、空の並びに変える。
- 切り出した枝の中では、元の引数をそのタグの定数で置き換える。
- 定数のタグの jump は、対応する枝の join point への jump にする。
- 元の join point の本体は、引数で分岐して枝の join point へ jump するだけになる。分からない値の jump は、今までどおり元の join point に来る。
- 入れ子の形は次のとおりである。枝の join point の範囲は、元の join point の定義全体になる。枝が使う外側の変数は、元の join point を定義した位置で範囲にある。

  ```
  join jT(u) { 枝 True } in
  join jF(u') { 枝 False } in
  join j(t) { switch t { #1 -> jump jT(()), #0 -> jump jF(()) } } in
  範囲 (定数のタグの jump は jT / jF へ)
  ```

### B3. jump が1つだけの join point を戻す

範囲の中に jump が1つしかない join point は、その jump を `let param = 値` と本体で置き換え、join point を消す。B2 で作った join point の引数は `()` を受けるだけで本体で使わないので、`let` を作らず本体だけを置く。jump の位置では、本体が使う外側の変数がすべて範囲にある。B2 の後では、元の join point の本体だけから jump される枝の join point がこの形になる。

### B4. 使われない join point を消す

jump がなくなった join point は、範囲だけを残して消す。B2 で、元の join point に来る jump がすべて定数のタグだった場合がこれに当たる。

### B5. 小さな本体を jump の位置に写す

本体が1命令だけの join point への jump を、その命令に置き換える。対象は、`return param`、定数の `return`、別の join point への `jump` である。別の join point への jump は、元の join point が行き先の範囲の中にあるので、jump の位置からも行き先に届く。

### B6. 守ること

- どの書き換えも、本体を「その jump に来たときだけ」実行される位置へ動かすだけである。エフェクトの順序と短絡評価は変わらない。
- 書き換えた後の `JoinId` は、関数の中で0から振り直し、`CoreFn::joins` を作り直す。

## テスト

[testing.md](../../implementation/testing.md) の種類で分ける。

### 段階A

- `eml_core_ir/tests/lower.rs` の join point を含むスナップショット (`pick`、`choose`、`around` など) に `captures` の欄が加わる (種類2)。`dup` と `decref` の位置と `saved` の並びは変わらない。変わったら、段階Aの誤りとして扱う。
- `eml_core_ir/tests/verify.rs` と `eml_interp/tests/run.rs` の手書きの IR に `captures` を足す (種類3)。期待値は変えない。
- verifier の新しいテストを `verify.rs` に足す。`captures` の書き漏れ、`Join` の時点で範囲の外の変数を `captures` に書いた場合、jump の時点で `captures` の RC の対象を所有していない場合を、それぞれ拒むことを確かめる。
- 呼び出しの後に jump する経路の `saved` が行き先の `captures` を含むことは、既存の `lower.rs` の `calls_save_the_variables_used_after_them` で確かめる (`call twice(s1) [t2]` と `join j0(t5) [t2]`)。`saved` を Perceus が埋めるようになっても、このスナップショットの `saved` の並びは変わらない。
- `lower.rs` に、範囲の中の join point の本体が外側の join point へ jump する関数のスナップショットを足す。内側の `captures` が、外側の `captures` を含むことを確かめる。
- 長い文の列の性能は、`eml_interp/tests/run.rs` の入れ子の join point のテストで確かめる (今あるテスト)。

### 段階B

- `simplify` の Core IR のスナップショットを、B2 から B5 のそれぞれについて足す。
- B0 で、条件を計算する末尾にない `if` を含むスナップショット (`calls_save_the_variables_used_after_them`、`ifs_in_a_condition_nest_join_points`) の束縛の位置が変わる (種類2)。
- 既存のスナップショットのうち、`simplify` で形が変わるものは種類2として記録する。`lower.rs` の `ifs_in_a_condition_nest_join_points` (`if (if a then b else False) then 1 else 2`) は B2 の対象そのもので、形が変わる。入れ子の join point を確かめる目的は、`simplify` を通らない形の新しいスナップショットで引き継ぐ。
- `tests/ui/run/` に、`&&` と `||` を `if` の条件に置き、右辺が `println` を呼ぶテストを足す。短絡評価とエフェクトの順序が保たれることを確かめる。今の `short_circuit.em` は `&&` を引数の位置で使うので、B2 の対象にならない。
- 既存の UI テストの出力は変わらない。

## 文書の更新

- `docs/spec/core-ir.md`: 制御の命令の表を `join j(x) [captures] { 本体 }` にする。join point の項に `captures` の意味を、Perceus の項に解析が1回であることを、verifier の項に `captures` を宣言として確かめることを書く。段階Bでは、マイルストーン1 で入れるパスに `simplify` を足す。
- `docs/implementation/architecture.md`: `liveness.rs`、`saved.rs`、`verify.rs` の説明を直し、段階Bで `simplify.rs` を足す。
- `docs/implementation/status.md`: 完了した作業に段階Aと段階Bを足す。
- `docs/implementation/testing.md`: 種類2の変更を記録する。
