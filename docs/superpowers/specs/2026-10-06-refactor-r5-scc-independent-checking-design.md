# リファクタリング R5: 型検査の SCC ごとの独立の設計

位置づけ: 作業用の設計文書。R5 を終えたら、残す価値のある内容を `docs/spec/types.md`、`docs/spec/diagnostics.md`、`docs/implementation/architecture.md`、`docs/implementation/testing.md`、`docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

R5 の目的は2つある ([status.md](../../implementation/status.md) の「R5 で直す項目」)。

1. 型検査の時間をプログラムの大きさに比例させる。
2. 型検査を、salsa のクエリにそのまま載せられる純粋な関数に分ける ([ロードマップ](../../future/roadmap.md) の「インクリメンタル化」)。

status.md の一覧は、「閉じたスキーム」「SCC ごとの使い捨ての表」「束のワークリスト」の3項目だった。検討の結果、この3項目だけでは目的1を満たせないことと、型の検査と Kind の推論をもっと深く分けられることが分かった。この spec はその結果を反映し、一覧より広い範囲を扱う。

### 計測

release ビルドの `eml check` で、合成プログラムを測った (2026-10-06)。

| 形 | 500個 | 1000個 | 2000個 |
|---|---|---|---|
| 多相な関数の連鎖 (`f_i x = f_{i-1} x`) | 0.03秒 | 0.08秒 | 0.28秒 |
| 独立した多相な関数 | 0.07秒 | 0.22秒 | 0.82秒 |
| `data` と `match` | 0.07秒 | 0.24秒 | 0.92秒 |
| 環状の相互再帰 (1つの SCC) | 0.15秒 | 0.60秒 | 2.42秒 |
| 持ち越しの連鎖 | 28.5秒 | 500秒 | 打ち切り |

持ち越しの連鎖は、`k_i x action = { action (); k_{i-1} x action }` (`k_i : a -> (Unit -> <e> Unit) -> <e> a`) を並べた形である。プロファイルでは、どの形も時間の大半が `Lattice::residual` と `carry_residual` で、持ち越しの連鎖では由来の比較 (`memcmp`) がそれに続いた。

### 検討で分かったこと

1. **持ち越しの制約が段ごとに増える。** `carry_residual` は、残す制約の重複を `(lin, mult, origin)` で除く。同じ `(lin, mult)` でも由来が違えば残るので、`k_i` のスキームの持ち越しの制約は i 個になり、具体化のたびにすべて複写される。`CarriedThrough { inner }` も、呼び出しの段数だけ入れ子になる。表を SCC ごとに分けても、この件は直らない。
2. **同じ違反を何度も報告している。** `keep` を3段重ねると、1つの使用に E3006 が3件出る。うち2件は表示が完全に同じである。[診断](../../spec/diagnostics.md) の E3006 は、同じ値を1件だけ報告すると定めている。
3. **大きな SCC では2乗が残る。** SCC の中の参照は Kind 変数を共有するので、環状の相互再帰では、すべての関数の `μ` が制約の輪でつながる。`generalize` は関数ごとに残す変数から辿るので、ほかの関数の変数を素通りして輪を1周する。
4. **由来が表を指している。** `KindReason::CarriedAcross` の `Across::Row` は表の `Row` を持つ。スキームに残す持ち越しの制約の由来もこれを含むので、そのままでは表から切り離せない。
5. **表を作るたびにモジュール全体の情報を作り直している。** `Table::new` は、`data_kinds` の不動点計算、名前の複製、多重度の表をその場で作る。
6. **本体の検査は Kind の解を読まない。** Kind について本体の検査がするのは、制約を足すことだけである。解を読むのは、違反の報告、`export` の矢印の線形性、スキームの `kinds:` の表示だけである。
7. **書き出した型の矢印の線形性は、誰も読んでいない。** Core IR は型から boxed の判定だけをし、`VarInfo::linearity` はつねに `Unr` である。`Type` の表示も線形性を出さない。
8. **呼び出し先について本体の検査が要るのは、型の形だけである。** 型の形はシグネチャだけで決まる。呼び出し先の本体の検査が要るのは、Kind の制約を具体化するときだけである。

6〜8から、型の検査と Kind の推論はデータの上で独立している。今の作りは、具体化のときに呼び出し先の Kind の制約を表へ複写するので、本体の検査が呼び出し先の SCC の完了を待つ。

### R5 に含めるもの

- 型検査を4つの純粋な関数に分ける (1章)。`Context`、段0 (`signatures`)、段1 (`check_body`)、段2 (`solve_scc`) である
- 閉じた型の形 `Shape`、Kind の問題 `KindProblem`、Kind のスキーム `KindScheme`
- 由来 (`KindOrigin`) を表から切り離す
- 段2のワークリスト、強連結成分による残す制約の計算、持ち越しの制約の組ごとの重複除去
- `Type` から `linearity` を除く
- 性能の受け入れテストと、ドキュメントの更新

### 範囲の外

- salsa の導入
- 複数のモジュール (S2)。閉じたスキームを `TypedModule` に公開するかは S2 で決める
- 推論のアルゴリズムの変更。本体の型検査の規則は変えない
- 非常に長い演算子の列でスタックがあふれる件
- 大きな SCC が多くの持ち越しの制約を持つ場合の時間 (3章の「残る限界」)

### ユーザーと合意済みの決定

- status.md の3項目に、持ち越しの制約の組ごとの重複除去と、`CarriedThrough` の `inner` の要約を足す (案A)。重複除去は E3006 の重複報告を直すので、外から見た振る舞いが変わる。既存のテストの期待値は変わらない。
- 案A の上に、本体の検査と Kind の解決を2段に分ける (案D)。案A の「SCC ごとの使い捨ての表で本体と Kind をまとめて検査する」形より、次の点でよい。
  - 本体を変えたときにやり直すのは、その本体の段1と、その SCC の段2だけになる。呼んだ側は段2だけをやり直し、Kind のスキームが変わらなければそれも省ける。案A では、呼んだ側の本体をすべて検査し直す。
  - 段1は関数ごとに独立しているので、並列に動かせる。
  - [型と Kind](../../spec/types.md) の「関数の本体はシグネチャだけを見て1つずつ検査できる」が文字どおりに成り立つ。
- 互換性は捨てる。`Type` から `linearity` を除き、同じ SCC の参照は変数の共有から等式に替える。
- R5 は1回で行う。段の構造と段2の速さの項目を分けない。
- 次の2案は採らない。
  - Kind をシグネチャに書かせる案。Kind の推論はこの言語の実験の中心であり、言語の設計を変える話になる。
  - 残す制約を、変数ごとの定数の上下限と変数どうしの順序の2つに絞る案。言わなくてよい制約まで残り、`kinds:` の表示が増えるだけで、意味の上で得るものがない。

## 1. 全体の構成

### 段と入出力

`eml_types::check` の中を、次の4つの純粋な関数に分ける。どれも可変の大域状態を持たず、引数だけで結果が決まる。

| 関数 | 単位 | 入力 | 出力 |
|---|---|---|---|
| `Context::new` | モジュール | `Module` | データ型の Kind、型とエフェクトの名前、エフェクトと操作の多重度 |
| `signatures` (段0) | 宣言ごと | `Module`、`Context` | 宣言ごとの `Shape` |
| `check_body` (段1) | 関数ごと | `Module`、`Context`、全宣言の `Shape`、関数の ID | `BodyTypes`、型の誤りの診断、`KindProblem` |
| `solve_scc` (段2) | SCC ごと | SCC の各関数の `KindProblem`、呼び出し先の `KindScheme` | 各関数の `KindScheme`、破れた制約の由来 |

宣言は、組み込み、操作、コンストラクタ、シグネチャのある関数である。

### 主なデータ

**`Shape`** は閉じた型の形である。シグネチャを型の表に下ろし (今の `lower` をそのまま使う)、表から切り離した木に写して作る。木の中の変数は、すべてスキームの中の番号で持つ。

- rigid な型変数: `Generics` の並びの番号と名前、Kind 変数 `μ` の番号
- rigid な row 変数: `Generics` の並びの番号と名前、Kind 変数 `σ` の番号
- 矢印の線形性: 定数か、Kind 変数 `m` の番号

Kind 変数の番号は、今の `Table::kind_vars` と同じ順に型をたどり、最初に現れた順に振る。線形性 (`μ` と `m`) と多重度 (`σ`) で番号の空間を分ける。`Generics` にあって型に現れない変数があれば、その Kind 変数は型に現れる変数の後に番号を振る。推論用の変数と継続の型はシグネチャから作る型に現れないので、`Shape` には入れない。

**`KindProblem`** は、1つの本体の型検査が出した Kind の制約の集まりである。番号は、段1の関数ごとの表の Kind 変数の番号をそのまま使う。

- 線形性の束と多重度の束の制約 (由来つき)
- 持ち越しの制約 (由来つき)
- 具体化の記録 (2章)
- 自分のシグネチャの Kind 変数の並び。`Shape` の番号から表の番号への対応である

**`KindScheme`** は多相化した後に残す制約で、番号は `Shape` の Kind 変数の番号である。持ち越しの制約の由来は、報告に使う1段の要約 `CarriedInner` だけを持つ。並びは正規形にする (3章)。同じ意味のスキームが同じ値になるので、salsa が「変わっていない」と判断して呼んだ側をやり直さずに済む。

**`TypedModule`** の公開する形は今と同じである (`signatures`、`bodies`、`builtins`、`operations`、`constructors`、`main`)。変わるのは `Type` から `linearity` が消えることだけである (2章)。

### ファイルの置き場所

| 置き場所 | 中身 |
|---|---|
| `context.rs` (新規) | `Context`。今 `Table` が持つモジュール全体の情報 (`data_kinds`、型とエフェクトの名前、エフェクトと操作の多重度) を移す |
| `shape.rs` (今の `scheme.rs` を作り替える) | `Shape`、表から `Shape` への写し、`Shape` から表への具体化 (多相な具体化と rigid な具体化)、`Shape` から `Type` への書き出しと `kinds:` の名前。`lower`、`lower_signature`、`lower_type`、`lower_constructor`、`lower_operation` と `Rigids` もここに置く |
| `check/` | 段1。`check_body` と、今の `body.rs`、`handle.rs`、`report.rs`、`equality.rs` |
| `kind/` (今の `kind.rs` を分ける) | `mod.rs` (`KindVar`、`Bound`、由来)、`problem.rs` (`KindProblem`、`KindScheme`、具体化の記録)、`solve.rs` (段2) |
| `table/` | 段1の作業領域。Kind については制約を集めるだけになる |

`usage.rs`、`carry.rs`、`exhaustive.rs`、`scc.rs`、`data.rs` の置き場所は変えない。

## 2. 段1: 本体の検査

### 表は関数ごとの作業領域にする

`check_body` は関数ごとに新しい `Table` を作る。`Table::new` は `&Context` を借りるだけにし、作る費用を関数の大きさに比例させる。表の Kind の束は、制約と由来を集めるだけの入れ物にする。解く処理 (今の `Lattice::solve`、`value`、`residual`、`Table::solve_kinds`、`lin_solution`、`carry_residual`) は段2へ移す。

### 自分のシグネチャ

自分の `Shape` を rigid な具体化で表に置く。rigid な具体化は、`Shape` の rigid な型変数を表の rigid 変数に、rigid な row 変数を表の rigid な row 変数に、Kind 変数を新しい変数にする。名前は `Shape` から写す。戻り値は `Rigids` (本体の注釈が使う `TypeVarId` と `RowVarId` から表への対応) と、自分の Kind 変数の並びである。部分適用の制約 (`closure_kinds`) は、今と同じく本体がある関数にだけ足す。

### 参照の具体化

トップレベルの値の参照は、どれも多相な具体化にする。関数、組み込み、操作、コンストラクタの値と、コンストラクタのパターンである。多相な具体化は、次の新しい変数を作る。

- rigid な型変数ごとに、新しい推論用の型変数。その `μ` も新しい Kind 変数にする
- rigid な row 変数ごとに、新しい row 変数。その `σ` も新しい Kind 変数にする
- 矢印の `m` ごとに、新しい Kind 変数

呼び出し先の制約は複写しない。代わりに具体化の記録を `KindProblem` に残す。

```rust
struct Instance {
    decl: Decl,                  // 関数、組み込み、操作、コンストラクタ
    lin: Vec<KindVar>,           // Shape の線形性の番号の順に、新しい Kind 変数
    mult: Vec<KindVar>,          // Shape の多重度の番号の順に、新しい Kind 変数
    origin: Option<KindOrigin>,  // 具体化したときに設定されていた由来
}
```

由来は、値の参照なら `Passed(name)`、コンストラクタのパターンなら `Unified` になる。今の `with_kind_origin` の設定をそのまま使う。

段1は、参照の先が同じ SCC かどうかを知らない。自分自身の再帰呼び出しも、ほかの参照と同じく記録する。区別は段2で行う。

次の2つは今のままにする。

- handler の操作の節は、HIR から `lower_operation` で下ろす。今も制約を複写していないので、記録は作らない。
- 本体の型の注釈は、`lower_type` で下ろす。

### 由来を表から切り離す

`KindOrigin` が表の `Ty` と `Row` を指さないようにする。これで `KindProblem` と `KindScheme` は比べられる純粋なデータになる。

- `KindReason::CarriedAcross` の `across: Across` を `multi: Option<OperationId>` に替える。持ち越しのパスが制約を作るときに、今の `report::multi_operation` と同じ規則で決める。row を束縛するのは同じ本体の検査だけで、持ち越しのパスはその後に動くので、報告のときに解く今のやり方と結果は同じである。`Across` は、持ち越しのパスの中で多重度の成分を作るために残す。
- `KindReason::CarriedThrough` の `inner` を、入れ子の `KindOrigin` から1段の要約に替える。

  ```rust
  struct CarriedInner {
      range: TextRange,
      label: InnerLabel,
  }

  enum InnerLabel {
      Kept(String),     // `x` is kept alive across this call
      Through(String),  // through this use of `f`
      Value,            // a value is kept alive across this call
  }
  ```

  文言は今の `carried_through` と同じである。`CarriedThrough` を作るのは段2の展開だけになる。
- `report::linear_misuse` は表を受け取らず、`Module` と由来だけで診断を作る。

### 段1の終わり

使用回数のパスと持ち越しのパスを今と同じく流した後、次の2つを作って表を捨てる。

- `BodyTypes`。書き出しに Kind の解は要らないので、この時点で確定する。
- `KindProblem`。表から制約、持ち越しの制約、具体化の記録、自分の Kind 変数の並びを取り出す。

### `Type` の変更

`Type::Fn` と `Type::Cont` から `linearity` を除く。表の `export` は解を読まなくなるので、`display` と1つにまとめる。`Type::Fn` と `Type::Cont` を組み立てるのは `eml_types` の中 (`check_main` と `ty.rs` の単体テスト) だけである。`eml_core_ir` は `Type::Fn { .. }` で照合するだけなので変わらない。`eml_core_ir` が再公開している `Linearity` は `VarInfo` が使うので残す。

将来 Perceus で `Lin` の値を RC の対象から外すときは、Kind の解を別の表として足す。

### 本体のない宣言の Kind の問題

組み込み、操作、コンストラクタは、`Shape` を rigid な具体化で使い捨ての表に置き、宣言から出る制約を足して `KindProblem` を作る。宣言から出る制約は、`closure_kinds` と、操作の引数の `unrestricted` である。`unrestricted` で固定しないエフェクトの型引数の Kind 変数は、rigid な具体化の対応から引く。今の `check_module` の冒頭の処理と同じ制約である。

## 3. 段2: Kind の解決

`solve_scc` は、SCC の各関数の `KindProblem` と、前の SCC の関数と組み込みなどの `KindScheme` を受け取る。返すのは、各関数の `KindScheme` と、破れた制約の由来である。表は使わず、整数の番号の上の束だけを扱う。

### 1. まとめる

SCC の各関数の `KindProblem` を、線形性と多重度のそれぞれについて1つの番号の空間に並べる。関数ごとに番号をずらすだけである。

### 2. 具体化の記録を展開する

- **参照先が同じ SCC の関数** (自分自身を含む): 記録の Kind 変数と参照先の自分の Kind 変数を、番号ごとに等式 (両向きの `≤`) で結ぶ。今の作りで変数を共有しているのと同じ解になる。変数どうしの制約は違反として報告されないので、由来は付けない。
- **参照先がそれ以外** (前の SCC の関数、組み込み、操作、コンストラクタ): 参照先の `KindScheme` の制約を、番号を記録の Kind 変数に置き換えて足す。由来は記録の由来である。持ち越しの制約は、記録の由来が `Passed(name)` なら `CarriedThrough { name, inner }` を由来にする。`inner` は `KindScheme` に残した要約で、範囲は記録の由来の範囲である。それ以外の由来はそのまま使う。今の `copy_carries` と同じ規則である。

### 3. ワークリストで解く

変数ごとの上向きの辺 (`v ≤ w`) の隣接表を1回だけ作る。すべての変数を最小元から始め、定数の下限 (`c ≤ v`) で上げた変数をワークリストに入れる。取り出した変数から上向きの辺に沿って値を伝える。束の高さは2か3なので、各変数が上がる回数は高々2回で、時間は制約の数に比例する。

解いた後、次の2種類を違反として集める。

- 定数の上限 (`l ≤ c`) で、`l` の値が `c` を超えたもの
- 持ち越しの制約で、`lin` の値が `Lin` で `mult` の値が `Multi` になったもの

返すのは、由来のある違反の由来だけである。並べ替えと重複除去は、`check_module` がモジュール全体でまとめて行う (4章)。

### 4. 残す制約を求める

線形性と多重度のそれぞれについて、変数どうしの制約のグラフ (下限から上限への辺) を Tarjan の方法で強連結成分に縮める。縮めた成分の DAG と、成分ごとの定数の上下限は、SCC で1回だけ作る。Tarjan は `scc.rs` と同じく明示的なスタックでたどり、Rust のスタックを使わない。

関数ごとに、自分の Kind 変数を含む成分を「残す成分」とする。残す成分ごとに、次の3つを出す。

- 成分の中に自分の変数が2つ以上あれば、それらが等しいことを、`Shape` の番号の順の輪 (`x₁ ≤ x₂ ≤ … ≤ x₁`) で表す。
- 上向きに DAG をたどり、残さない成分を通り抜けて最初に出会った残す成分への辺 (`代表 ≤ 代表`) を出す。代表は、成分の中で `Shape` の番号が最小の自分の変数である。成分自身と通り抜けた成分の定数の上限の最小を、成分の中の自分の変数すべてに付ける。
- 下向きに同じようにたどった定数の下限の最大を、最小元でなければ、成分の中の自分の変数すべてに付ける。

残す変数どうしが同じ循環に入らない場合、この規則は今の `Lattice::residual` と同じ結果になる。今の `residual` は、残す変数で止まりながら1つずつたどる。同じ循環に入る場合は、今の結果はたどる順で変わり、定数の境界がどの変数に付くかが偶然で決まる。新しい規則では、等しい変数のすべてに同じ境界が付く。

環状の相互再帰では、輪のすべての `μ` が1つの成分に縮むので、関数ごとに輪を1周することがなくなる。

**持ち越しの制約**は、SCC の持ち越しの制約を1つずつ見て、両側を下向きにたどる。今の `carry_residual` と同じく、残す成分で止まり、そこまでに出会った定数を集める。

- 両側を、出会った残す成分の中の自分の変数と、定数に置き換える。定数は、値の側なら `Lin`、row の側なら `Multi` だけを残す。片側が残す成分の変数なら、その成分の中の自分の変数すべてに置き換える。
- 置き換えた組の直積を作り、両側が定数の組は捨てる。その本体の中の違反で、3.で報告済みだからである。
- 同じ `(lin, mult)` の組は1つにまとめる。由来は、範囲の始まり、終わりの順で最も前のものを残す。由来のない組は、由来のある組に負ける。
- 残す由来は `CarriedInner` に要約する。`CarriedAcross` の局所変数と `return` の節の捕獲は `Kept(name)`、`CarriedThrough` は `Through(name)`、それ以外は `Value` にする。

### 5. 正規形

`KindScheme` の制約は、次の順に並べた正規形にする。

- 線形性と多重度の制約: 変数側の番号の順に、変数どうしの制約 (相手の番号の順)、定数の上限、定数の下限
- 持ち越しの制約: `(lin, mult)` の順。変数は番号の順で、定数より前に置く

`kinds:` の表示は、定数を片側に持つ線形性の制約と持ち越しの制約だけを出す。今の表示も、スキームの変数の順 (`kind_vars` の順) に、上限、下限の順で出している。`Shape` の番号はこの順と同じなので、表示の並びは変わらない見込みである。

### 残る限界

1つの SCC の中の時間は、制約の数に、各関数が残す成分から残さない領域をたどる量を足したものになる。前の SCC の制約は見ない。残る限界は次の2つで、どちらも大きな SCC が多くの持ち越しの制約を持つ場合などにだけ起きる。status.md の既知の制限に書く。

- DAG の形によっては、残さない成分を関数ごとに何度もたどる。
- 持ち越しの制約を関数ごとに SCC 全体から見るので、関数の数と持ち越しの制約の数の積がかかる。

## 4. `check_module` の組み立て

### 流れ

```
check_module(module):
  context = Context::new(module)
  shapes  = signatures(module, &context)                 // 段0。宣言ごと
  schemes = 組み込み・操作・コンストラクタの KindScheme      // 段2を1宣言ずつ
  diagnostics = check_main(module, &shapes)              // E2004
  for scc in components(module):                         // 呼ばれる側から
    for f in scc: 段1 → bodies[f], diagnostics に型の誤り, problems[f]
  for scc in components(module):
    段2 → schemes に SCC の関数の KindScheme, violated に由来
  diagnostics += report(violated)                        // 位置の順
  typed = TypedModule を shapes と schemes と bodies から作る
  diagnostics += exhaustive::check(module, &typed)
```

段1は関数ごとに独立しているので、どの順に呼んでも結果は同じである。SCC の順に呼ぶのは、型の誤りの診断の並びを今と同じにするためである。

シグネチャのない関数は、今と同じく検査しない。参照すると `Error` の型になり、具体化の記録も作らない。シグネチャだけで本体のない関数の `KindScheme` は空である。

### 診断の順

今の順を保つ。

1. E2004。`main` の `Shape` を `Type` に書き出し、`Unit -> <IO> Unit` と比べる。表は使わない。シグネチャに誤りの跡があれば報告しない規則 (`has_error`) は残す。
2. 本体ごとの型の誤り。SCC の順に並べる。
3. Kind の違反。すべての SCC の由来を集め、範囲の位置の順に並べて同じ由来を除く。次に、同じ値の持ち越しの違反を最初の1件に絞り (`CarriedValue::key`)、`linear_misuse` で診断にする。今の `solve_kinds` と `check_module` の処理を、表なしで行う形である。由来の範囲はその SCC の本体の中にしかないので、SCC ごとに解いても、全体を並べ直せば今と同じ順になる。
4. 網羅性の誤り。

### `TypedModule` の書き出し

- `signatures`、`builtins`、`operations`、`constructors` の `Scheme` は、`Shape` と `KindScheme` だけから作る。
  - `ty` は `Shape` を `Type` に書き出したものである。
  - `constraints` は今の `kind_constraints` と同じ規則で作る。定数を片側に持つ線形性の制約と、持ち越しの制約を表示用にする。
  - 変数の名前は `Shape` から引く。rigid な型変数の `μ` は型変数の名前、矢印の `m` はその矢印の型の表示、row の `σ` は row 変数の名前である。今の `kind_names` と `row_names` と同じ規則である。
- `bodies` は段1の `BodyTypes` をそのまま入れる。

`Shape` と `KindScheme` は `TypedModule` に入れない。S2 でモジュールの間に渡すときに、公開するかを決める。

## 5. テスト

### 既存のテストの変更

| テスト | 変更 | 種類 |
|---|---|---|
| `tests/ui/` のすべて | 変えない。バイト単位で同じ出力になることを受け入れの条件にする | なし |
| `eml_types/tests/` の dump (`kinds:` の行) | 変わらない見込み。残す変数どうしが同じ循環に入る関数で表示が変わったら、変わったテストと理由を test-changes.md に書く | 2 (起きたときだけ) |
| `table/tests.rs` の組み立て | `Table::new` が `&Context` を受け取る形に追随する | 3 |
| `table/tests.rs` の Kind を読む3件 (`an_open_row_absorbs_the_missing_labels`、`binding_a_variable_passes_the_kind_of_its_type_to_the_variable`、`closure_kinds_bound_each_partial_application`) | 表から解や残す制約を読む代わりに、表の制約を段2の解き方に渡して読む。`assert` の値は変えない | 3 |
| `table/tests.rs` の `export_needs_solved_kinds` | 削除する。「解く前に書き出せない」という性質そのものがなくなる | 1 |
| `table/tests.rs` の `display_does_not_solve_kinds` | 削除する。`export` と `display` を1つにまとめるので、確かめる対象がなくなる | 1 |
| `kind.rs` の単体テスト9件 | `kind/solve.rs` へ移し、新しい API で組み立て直す。`assert` の値は変えない | 3 |
| `ty.rs` の単体テスト | `Type` の組み立てから `linearity` を除く。表示の文字列は変えない | 3 |

`residual_constraints_pass_through_internal_variables` は、残す制約の並びを `[(a, b), (a ≤ Unr)]` と確かめている。3章の正規形 (変数どうしの制約を定数の制約より前に置く) は、この値を保つように決めた。

### 新しいテスト

- `eml_types/tests/linearity.rs`: 持ち越しの制約が3段の多相な関数を通るとき、E3006 が1件だけ出る。secondary は、一番外側の関数の中で位置が最も前の由来 (`action ()` の呼び出し) を指す。UI テストには足さない。1段の代表的な筋書きは `file_through_polymorphic_function.em` が確かめており、端のケースは段階の crate に置く決まりだからである ([testing.md](../../implementation/testing.md) の「重複させない」)。
- `kind/solve.rs` の単体テスト:
  - ワークリストが鎖をたどって値を伝える
  - 強連結成分に入った残す変数のすべてに同じ境界が付く
  - 同じ SCC の具体化の記録が等式になる
  - 前の SCC の具体化の記録で `KindScheme` の制約が複写され、持ち越しの制約の由来が `CarriedThrough` になる
  - 持ち越しの制約を組ごとに1つにまとめ、位置が最も前の由来を残す
- `eml_types/tests/check.rs`: 環状の相互再帰の関数の `kinds:` の表示。
- `shape.rs` の単体テスト: 表から `Shape` へ写して rigid な具体化で戻すと、`kind_vars` の順と `Shape` の番号が一致する。

### 性能の受け入れテスト

- 置き場所は `crates/eml_types/tests/scaling.rs` にする。テストに `#[ignore]` を付け、`cargo test --release -p eml_types --test scaling -- --ignored` で流す。
- 合成プログラムの生成器をこのファイルの中に置く。形は、多相な関数の連鎖、独立した多相な関数、`data` と `match`、持ち越しの連鎖、環状の相互再帰の5つである。
- `eml_test_support` で HIR まで作ってから、`eml_types::check` の時間だけを測る。各大きさで3回測り、最小の値を使う。
- 関数の数を 2000 と 8000 にし、時間の比が6以下であることを確かめる。4倍の大きさに対し、ばらつきとハッシュ表の伸びの分の余裕を見込んだ閾値である。
- 生成したプログラムは、診断なしで型検査を通ることも確かめる。
- 今の実装では、持ち越しの連鎖が 2000 で終わらず、環状の相互再帰の比はおよそ16倍なので、このテストは失敗する。

### 受け入れの条件

- `cargo test` がすべて通る。UI テストのスナップショットは変わらない。
- 性能の受け入れテストが通る。
- `cargo clippy --all-targets` が警告を出さない。

## 6. ドキュメントの更新

| 文書 | 更新する内容 |
|---|---|
| `docs/spec/types.md` の「推論」 | 本体はシグネチャの形だけを見て検査すること。SCC の間で流れる情報は Kind のスキームだけであること (シグネチャが必須だから)。同じ循環に入った Kind 変数の扱い。持ち越しの制約を組ごとに1つにまとめる規則 |
| `docs/spec/diagnostics.md` の E3006 | スキームを通る持ち越しの違反は、組ごとに位置が最も前の由来を指すこと |
| `docs/implementation/architecture.md` | `eml_types` の中の段0〜2と、それぞれの入出力 |
| `docs/implementation/status.md` | R5 を完了にする。「性能: `Lattice::residual` …」の項を消し、3章の「残る限界」を既知の制限として足す。`eml_types` の説明を直す |
| `docs/implementation/testing.md` | `scaling.rs` と流し方、テストの地図の `eml_types` の行 |
| `docs/implementation/test-changes.md` | R5 の節。種類1の2件の削除と、起きた場合の種類2 |
