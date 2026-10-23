# never-rows

- 作成日: 2026-10-24
- 基点: main @ 77ce3360。ファイルの行番号はこの時点で読んだもので、探す手がかりにとどめる。関数や節の名前で引く
- 状態: 計画は確定。決めていない細部は 10 章に既定とともに置く

この計画は docs/future/roadmap.md の段 `never-rows` の設計と計画である。roadmap の「進め方」に従い、この文書を段の最初のコミットで入れ、段を閉じるときに消す。コードと `docs/` はこの文書を引かない。

## 1. 目的と成功の基準

- 目的
  - extern のエフェクト `Panic` の多重度を `Never` にする。`IO` は `Once` のままにする
  - シグネチャの row 変数に多重度の上限 `never` を書けるようにする (`<never e>` は `e : Row<Never>`)。書かなければ、呼び出しから推論で付く
  - 上限を超える row を入れる呼び出しを、新しい型の誤り E2020 にする
  - 次の段 `par-core` は、この上に `par : (Unit -> <never e> a) -> (Unit -> <never e> b) -> <e> (a, b)` を入れる
- 成功の基準 (確かめ方は 9 章)
  - multicore.md と async.md に書いてある `par`、`Par.map` などの `<never e>` を使うシグネチャの形が、すべて読める
  - `Panic` を起こす関数を `<never e>` の引数に渡せて、`IO` を起こす関数を渡すと E2020 になる
  - 既存のテストの期待値が1つも変わらない
  - 全体の検証のコマンドがすべて通る

## 2. 範囲

- 含むもの
  - `Panic` の多重度 (D-4、T-1)
  - 構文 `'never'? LIDENT` (D-1、T-2)
  - 書ける場所と E1057 (D-2、T-2)
  - Kind のスキームの上限の制約 (D-3、T-3)
  - E2020 と、推論した上限の出どころの表示 (D-7、T-3、T-4)
  - 上限から導ける持ち越しの制約を残さないこと (D-6、T-5)
  - UI テスト (T-6)
  - 段を閉じる作業 (T-7)
- 含まないもの
  - `par`、`both`、`std/Par`、実行系。次の段 `par-core` で入れる
  - `<once e>`、`data` の row の型引数の上限、操作のシグネチャの上限、`:type` での上限の表示。T-7 で roadmap の「その後の項目」へ移す
  - Kind の束への `Tail` の挿入 (multicore.md の「未決の論点と将来の拡張」)

## 3. 背景

- 構文
  - `never`、`once`、`multi` は字句の上でいつもキーワードである (`NEVER_KW` などの3つ。crates/eml_syntax/src/lexer/mod.rs:675-677)
  - 今の row の文法は `row ::= '<' '>' | '<' LIDENT '>' | '<' effect (',' effect)* ('|' LIDENT)? '>'` である (docs/spec/grammar.md:107)。`<never e>` は今は E0011 (expected an effect) になる
  - `grammar/types.rs` の `effect_row` は、`p.at(LIDENT)` のときだけ row 変数1つの row を読み、`|` の後ろは `expect(p, LIDENT)` で読む。`row_param` (`data T <e>`) は LIDENT 1つだけを受ける
  - `OpDecl::multiplicity()` は、子のトークンから多重度のキーワードを探す。`EffectRow::tail()` は LIDENT の子を返す
- HIR
  - `lower/types.rs` の `Vars` は3種類である。`Define` はシグネチャの形で、名前を暗黙に量化する。`Signature` は本体の型の明示の形で、シグネチャの名前だけを使える (ほかは E1002)。`Data` は `data` のフィールドの形で、宣言の型引数だけを使える
  - 1つのシグネチャの同じ名前の row 変数は、1つの `RowVarId` である。`RowVarDecl` は `name` だけを持つ (crates/eml_hir/src/hir.rs:382)
  - 操作のシグネチャは今 `Vars::Define` で下ろす (lower/effect.rs:107)
  - HIR の誤りの番号は E1056 まで使っている (crates/eml_hir/src/lib.rs:111-116)。次は E1057 である
- 型検査
  - 多重度の束は `Multiplicity { Never, Once, Multi }` である (ty.rs:13)。row 変数はそれぞれ多重度の Kind 変数 σ を持つ (`RowVarInfo.multiplicity`、table/mod.rs:178-185)
  - 多重度の制約を作るのは2か所だけで、どちらも下限である。`bind_row` (table/row.rs:279-299) はラベルごとに `Const(ラベルの多重度) ≤ σ` を足し、末尾があれば `σ_末尾 ≤ σ` を足す。`multiplicity_at_least_multi` (check/handle.rs:110) は操作の節の row 変数に `Multi ≤ σ` を足す
  - 上限 `σ ≤ Const` を作る所は今どこにもない。ただし solver (`residual`、`copy_scheme`、entail.rs) はこの形を一般の形として扱える
  - Kind はシグネチャから決まらず、いつも推論する。宣言した Kind と推論した Kind を比べるのは、クラスのメソッドだけである (`check_method_kinds` と entailment、E2011)
  - 持ち越しの制約は条件付きの `Carry{lin, mult, origin}` で (kind/mod.rs:543)、`Lin` かつ `Multi` のときだけ破れる。`Never` と `Once` は同じに扱う
  - 上限が破れた多重度の制約は、今は理由を持たず `linear_misuse` に落ちて E3001 と表示される (report.rs:732)
  - `solve_scc` は solver が返す原因を捨てている (kind/solve.rs:303-306)。`solve_with_causes` (:309-345) は、変数ごとに値を決めた定数の下限の制約を返す
  - 同じ SCC の参照は `Merged::equate` (kind/solve.rs:224-240) が、インスタンスの σ と自分の σ を両向きに等しくする
  - 型検査の誤りの番号は E2019 (`VAR_ESCAPES`、crates/eml_types/src/lib.rs:67) まで使っている。次は E2020 である
  - `Context::new` (context.rs:30-70) は、extern のエフェクトの多重度を一律に `Once` にしている (`EffectKind::Extern(_) => Multiplicity::Once`)。`var` のエフェクトも `Once` である
  - extern のエフェクトであることで決まる扱い (重なったラベルを1つにまとめる、`mask` に入れない、`may_yield`) は、多重度を見ない (table/row.rs:27、:433、context.rs:80)
  - 型の表示は row を `<l, l | tail>` と書き、Kind も上限も書かない。REPL の `:type` も同じ表示を使う。Kind の書き出しは線形性だけで、「多重度は書き出さない」(docs/spec/types.md の「Kind の書き出し」)
- `eml_extern` の `EffectRow` は `name` だけを持つ (crates/eml_extern/src/lib.rs:226-241)。`eml_extern` は依存を持たない crate なので、`eml_types::Multiplicity` を使えない
- 下流の crate (Core IR、LIR、VM、CLI) は、エフェクトの多重度を読まない
- 計画の段階の試作で確かめたこと (試作はリポジトリの外で行い、消してある)
  - `Panic` を `Never` にしても、既存のテストはすべて通る (`cargo nextest run --workspace` で 3512 passed、18 skipped)
  - この計画の制約の設計を入れた試作でも、既存のテストの期待値は1つも変わらない (3512 passed、18 skipped)。試作は E2020 の表示を持たず、違反は E2020 を出す位置に E3001 として出た
- multicore.md と async.md に書いてある、後の段で入れるシグネチャ (この段の構文で読めなければならない)

  ```haskell
  par     : (Unit -> <never e> a) -> (Unit -> <never e> b) -> <e> (a, b)   -- multicore.md の「`par` の意味」
  compute : (Unit -> <never e> a) -> <IO, Async | e> a                       -- async.md の「`compute`」
  ```

  multicore.md の「データ並列での使い勝手」の `Par.map` などもすべて引数の矢印に `<never e>` を書き、`tabulate` の結果の row は `<Panic | e>` である。

## 4. 前提の環境

| 要るもの | 使う所 | 確かめ方 | ないとき |
|---|---|---|---|
| Nix の devShell (Rust 1.98.1、cargo-nextest、cargo-insta) | すべて | `cargo nextest --version` と `cargo insta --version` が版を出す | 止めて利用者に聞く |
| `.#cross` の shell | T-7 | `nix develop .#cross -c cargo --version` が版を出す | 止めて利用者に聞く |
| 基点のテストが通ること | すべて | 9 章の基準のコマンド | 止めて利用者に聞く |

この段は `crates/eml_vm`、`eml_rt`、`eml_lir`、`eml_core_ir`、`std/`、`bench/` を変えないので、Miri、Verus、`bench/*.sh` は要らない (D-15)。

## 5. 決めたこと

### 5.1 長く効く方針 (変えるときは設計からやり直す)

実装の途中でこれらとぶつかったら、止めて報告する。

#### D-1: 上限は row 変数の前に書く `never` だけ

- 決定
  - 文法は次のとおりである。`tparam` と `op_decl` は変えない

    ```
    row     ::= '<' '>' | '<' row_var '>' | '<' effect (',' effect)* ('|' row_var)? '>'
    row_var ::= 'never'? LIDENT
    ```

  - 読める形: `<never e>`、`<Panic | never e>`、`<IO, Async | never e>`、型の適用の row の引数 (`Iter <never e> a`)
  - E0011 にする形
    - `never` の後ろが row 変数でない形 (`<never Panic | e>`、`<never>`)。メッセージは「expected a row variable」で、row の残りはエフェクトとして読み続ける
    - `never` 以外の多重度 (`<once e>`、`<multi e>`)。メッセージは「only `never` can bound a row variable」で、キーワードを読み飛ばす
    - `data` の row の型引数の上限 (`data T <never e>`)。メッセージは「a row parameter of `data` cannot have a bound」で、キーワードを読み飛ばして `e` を型引数のまま残す
  - どの形も E0011 を1件だけ出し、ほかの誤りを続けて出さない
- 理由
  - Kind は row 変数のものであり、後の段のシグネチャもすでにこの形で書いてある
  - 変数に付けるので、`<IO | never e>` (`IO` を含む row で、`e` だけを `Never` にする) が書ける。`compute` にはこれが要る
  - `never` だけにするのは、使う段があるのが `Never` だけだからである。`once` の上限を入れると `once` の語に3つ目の意味ができ、`Tail` を束に挿入するかを決める前に `once` の読みを固めてしまう。`multi` は束の最大元で、何も制限しない
- 退けた案
  - row 全体に付ける形 (`<never Panic | e>`): ラベルを確かめる規則と、同じ語の2つ目の意味が要る
  - 文脈の制約の形 (`never e => …`): 文脈の構文は `signature` と `op_decl` にしかなく、後の段のシグネチャとも合わない
  - `<once e>` も今入れる: 使う段がなく、`Tail` が決まっていない
- 見直す条件: `<once e>` を使う段ができ、`Tail` を入れるかが決まったとき。足すのは文法の1トークンで済む
- リポジトリに残す所: grammar.md の `row` の規則と「文法上の補足」。T-2 で書く

#### D-2: 上限はシグネチャで宣言し、同じ変数の出現はどれも同じ上限を持つ

- 決定
  - 上限を宣言できるのは、row 変数を量化するシグネチャ (トップレベルの関数と値、extern の関数、クラスのメソッド) だけである。その変数のどの出現 (一番外側の矢印、内側の矢印、型の適用の row の引数) に書いてもよい
  - 1つのシグネチャの同じ名前の row 変数は1つの変数で、`never` を書かなかった出現も同じ上限を持つ。そのため `par` の結果の row `<e>` も `Never` である
  - 子の row だけを制限し、続きの関数の row を制限しないときは、別の row 変数を使う (`(Unit -> <never e1> a) -> (a -> <e> b) -> <e> b`)。子の `never` の操作を結果の row に流すときは、`<Panic | e>` のようにラベルで書く
  - 上限を持つ row 変数が呼び出し先の結果の row にあっても、呼び出す側の row は制限されない。呼び出しの包含は呼び出し先の末尾を今の row と単一化せず、末尾の多重度を今の row の多重度に引き上げないためである (types.md の「呼び出しの包含を解く」)。`<IO, Panic>` の本体からも `par` を呼べる
  - 本体の型の明示 (`(x : T)`、`let x : T`、`var x : T`、ラムダの引数の型) はシグネチャの変数を指す。シグネチャが `never` を付けた変数にだけ `never` をくり返して書ける。付けていない変数に書いたら E1057 である
  - `data` のフィールドの row 変数は宣言の型引数で上限を持たないので、`never` を書いたら E1057 である
  - 操作のシグネチャにも書けない (E1057)。操作ごとの row 変数は節の中で `Multi` 以上になるので (effects.md の「handler の意味」)、上限と両立しない
- 理由
  - 宣言の型の形はシグネチャだけで決まる (types.md の「推論」)。上限をほかの所で増やせると、この規則が崩れる
  - 本体の注釈でくり返せると、`Par` の補助のコードが同じ変数を同じ形で書ける
  - 1つの変数は1つの Kind を持つ。後の段のシグネチャは、結果の `<e>` に `never` を書かない形に頼っている
- 退けた案
  - 最初の出現にだけ書かせる: 後の段のシグネチャを退けるか、書く順に意味を持たせてしまう
  - 本体の注釈では一切書かせない: 「どの出現に書いてもよい」と食い違い、局所の補助関数で不便である
  - `data` の row の型引数に上限を許す: 型の適用ごとに制約を確かめる仕組みが要り、`par` には要らない
- 見直す条件: `data Task <never e> a` のような型を使う段ができたとき
- リポジトリに残す所: types.md の新しい節「row 変数の多重度の上限」(書ける場所と E1057 の文は T-2、意味の文は T-3)

#### D-3: 上限は宣言の Kind のスキームの制約で、違反はいつも呼び出しで起きる

- 決定
  - シグネチャに書いた上限は、本体を検査する前に、その宣言の Kind の問題に `σ_e ≤ Never` として足す。本体が求めなくてもスキームに残り、参照するたびに複写される
  - 書いた制約と推論した制約は、スキームの中で区別しない。書かなかった上限は本体から推論する。上限を持つ row 変数に引数の row を入れる呼び出しは、その変数の多重度を引数の多重度以上にする。そのため `par` に渡す関数を受け取る関数は、書かなくても同じ上限をスキームに持つ (multicore.md の「シグネチャを書かなければ推論で付く」)
  - 違反はいつも呼び出しで起きる。ラムダ、handler の節、ローカル関数、今の row のどれから来た row でも、上限を持つ row 変数に入れた呼び出しを指して E2020 にする
  - 自分の本体の中で上限を破るのは、自分を (同じ SCC の関数を通して) 呼ぶ呼び出しだけで、それもほかの呼び出しと同じく E2020 にする
- 理由: 2段階の Kind 推論の形を変えずに済む。新しく入るのは定数の上限1つで、solver はそれを一般の形として扱える
- 退けた案: 上限を参照ごとに足す (`Shape::instantiate`)。SCC をまたぐ呼び出しで、理由の違う制約が2つずつできる
- 見直す条件: Kind の宣言化 (roadmap の「その後の項目」) を入れるとき
- リポジトリに残す所: types.md の「推論」の下位の箇条。T-3 で書く

#### D-4: `Row<Never>` に入るものと、extern のエフェクトの多重度

- 決定
  - row の多重度は、ラベルのエフェクトの多重度と、末尾の row 変数の多重度の上限 (join) である
  - `Row<Never>` に入るもの: `<>`、`Panic`、操作がすべて `never` のエフェクト (`Std.Fail` の `Fail x`)、操作のないエフェクト、`never` の上限を持つ row 変数
  - 入らないもの: `IO` (`Once`)、`var` のエフェクト (`Once`)、`once` か `multi` の操作を持つエフェクト
  - extern のエフェクトの多重度は、extern の型の Kind と同じく `eml_extern` の表の行が決める。表の決め方は次のとおりである
    - 起こすと呼び出し元へ戻らず、外の世界に何も残さないエフェクトは `Never` にする。`Panic` がこれに当たる
    - extern の関数がその場で1回だけ戻るか、外の世界への書き込みが順序を持つエフェクトは `Once` にする。`IO` がこれに当たり、並列のタスクの row に入らない
- 理由
  - どちらの操作も `never` の操作 (`io_failed`、`panicked`) だけなので、操作から計算すればどちらも `Never` になる。そのため、決め方を規則として書いておかないと、extern のエフェクトを足すたびに同じ議論になる
  - extern の事実は1つの表に置く (CLAUDE.md の不変条件)。`eml_types` の中で名前を見て決める特例は、表の外に置かれた唯一の extern の事実になってしまう
- 退けた案: `Context::new` で `Panic` だけを名前で `Never` にする。試作では動いたが、表の外に事実を置く
- 見直す条件: extern のエフェクトを足すとき、または `Tail` を束に挿入するとき
- リポジトリに残す所: types.md の「Kind」、effects.md の「組み込みの `IO`」と「組み込みの `Panic`」。T-1 で書く

#### D-5: 上限は表示しない

- 決定
  - 「多重度は書き出さない」を保つ。型の表示、診断、REPL の `:type` は、上限を書いたシグネチャでも `<e>` と書く。書いた上限も推論した上限も同じである
  - REPL はセッション全体を入力ごとに検査し直すので (modules.md の「REPL のセッション」)、上限を入力の間で持ち越す仕組みは要らない
- 理由
  - 後の段階は多重度を読まない
  - 書いた上限だけを表示すると、同じスキームの2つの関数が違って表示される
  - 上限の情報が要るのは違反した所なので、E2020 の secondary が伝える
- 退けた案: `:type` で最初の出現に上限を書く。表示のためだけに多重度の書き出しが要る
- 見直す条件: Kind の宣言化で Kind を一般に表示するようになったとき
- リポジトリに残す所: types.md の「書き出す型の形」。T-3 で書く

#### D-6: 上限から導ける持ち越しの制約は残さない

- 決定
  - 多相化するとき、上限が `Never` の自分の row 変数を row の側に持つ持ち越しの制約 (`carry`) は、スキームに残さない。上限から導けるからである
  - クラスのメソッドでも同じで、クラスの側のスキームの上限から、instance の持ち越しの制約が導ける (entail.rs:44-49 は、上限が `Once` 以下の持ち越しを満たされたものとして扱う)
- 理由: 残すと、上限を超える row を入れた1つの呼び出しが、E2020 と E3006 の両方になる
- 退けた案: 持ち越しを残し、報告の段で E3006 を抑える。同じ結果に、より多くのコードが要る
- 見直す条件: なし
- リポジトリに残す所: types.md の「推論」の2つ目の下位の箇条。T-5 で書く

#### D-7: 誤りの番号と段階

| 場面 | 段階と番号 | 指す所 |
|---|---|---|
| `<never Panic \| e>`、`<never>`、`<once e>`、`<multi e>`、`data T <never e>` | 構文、E0011 (今の番号) | `never` の後ろのトークン、またはキーワード |
| 本体の注釈か `data` のフィールドで、`never` を宣言していない変数に `never` を書いた。操作のシグネチャに `never` を書いた | HIR、E1057 `ROW_BOUND_NOT_DECLARED` (新しい番号) | キーワード |
| 呼び出しが、上限 (書いたものと推論したもの) を超える row を入れた | 型検査、E2020 `ROW_BOUND_EXCEEDED` (新しい番号) | 呼び出し先への参照。1つの呼び出しに1件 |
| instance か既定のメソッドが、クラスのシグネチャにない上限を求める | 型検査、E2011 (今の番号) | 今と同じ。note と help だけを足す (D-13) |

- リポジトリに残す所: docs/spec/diagnostics.md と docs/implementation/diagnostics.md の表。E1057 は T-2、E2020 は T-3 で書く

### 5.2 短期から中期の設計

理由があれば実装の途中で変えてよい。変えたときは記録する。

#### D-10: 構文と CST

- `grammar/types.rs` に `fn row_bound(p) -> bool` を足す
  - `NEVER_KW` なら読んで true を返す
  - `ONCE_KW` か `MULTI_KW` なら、E0011「only `never` can bound a row variable」(ラベル `found {describe(p)}`) を出し、`bump_any` して true を返す
  - それ以外なら false を返す
- `effect_row` の空でない枝では、まず `let bounded = row_bound(p);` とする。次が `LIDENT` なら読む。そうでなく `bounded` なら `expected(p, "a row variable")` を出し、今と同じくエフェクトとして読む。`|` の後ろでは、`expect(p, LIDENT)` の前に `row_bound(p)` を呼ぶ
- `row_param` では、`well_formed` の前に `NEVER_KW | ONCE_KW | MULTI_KW` があれば、E0011「a row parameter of `data` cannot have a bound」(ラベル `found …`) を出して `bump_any` する
- キーワードは `EFFECT_ROW` か `ROW_PARAM` の直接のトークンの子にする。`EffectRow::bound() -> Option<SyntaxToken>` を足し、`NEVER_KW` のトークンを返す。`tail()` は変えない

#### D-11: HIR

- `RowVarDecl` を `{ name: String, never: Option<TextRange> }` にする。範囲は、そのシグネチャでその変数に最初に書いた `never` である。`lower/data.rs:31` は `never: None` にする
- `lower/types.rs` の `Vars` に `Operation` を足す。名前の取り方は `Define` と同じで、`type_var` と `row_var` は `Define | Operation` を受け、`undeclared` に枝を足す。`lower/effect.rs:107` はこれを使う
- `row_var(name, bound)` の `bound` は `row.bound()` の範囲である
  - `Operation` で上限があれば E1057 を出し、上限を捨てる
  - `Define` で上限があれば、見つけた変数の `never.get_or_insert(at)` とする。新しく取る変数なら、そのまま設定する
  - `Signature` か `Data` で上限があり、変数の `never` が `None` なら、E1057 を出し、上限を無視する
- E1057 `ROW_BOUND_NOT_DECLARED = ErrorCode(1057)` は、`never` のトークンを主な範囲にし、メッセージを「`never` cannot bound `{e}` here」にする。secondary は付けない (付録の C-3)

  | 場面 | ラベル | help |
  |---|---|---|
  | `Signature` | the signature does not bound `e` with `never` | write `never e` in the signature, or remove `never` here |
  | `Data` | a row parameter of `data` has no bound | remove `never` |
  | `Operation` | the row variables of an operation cannot be bounded | remove `never` |

- `pretty.rs:684` は、上限を持つ row 変数のどの出現の前にも `never` を書く
- instance のメソッドは、写した `row_vars` (instance.rs:556) を通して、クラスの上限を受け継ぐ

#### D-12: `eml_extern` と `Context`

- `eml_extern` に `pub enum EffectMultiplicity { Never, Once }` を足し、`EffectRow` を `{ name, multiplicity }` にする。`Io => Once`、`Panic => Never` である。`ExternEffect` の doc コメントは「表の行は名前と多重度を持つ」にする。`EffectRow` を作る所と読む所は、ほかにない
- `context.rs:46` の枝は `EffectKind::Extern(e)` で `e.map(|e| e.row().multiplicity)` を引き、`Never` を `Never` に、`Once` と `None` を `Once` にする。`None` は知らない extern で、すでに誤りを出している

#### D-13: 型検査の制約

- 自分の上限を足す所は1か所だけで、`Shape::instantiate_rigid(&self, table, generics, file: FileId)` にする
  - `generics.row_vars` の各項目の `never` が `Some(range)` なら、`σ ≤ Never` を `Provenance::Bound(Span { file, range })` で足す
  - `Shape` は位置を持たないままにする
  - 呼び出す所は `check_body` (check/mod.rs:481)、`declaration_problem` (`file: FileId` を引数に足す。`declaration_schemes` が extern の関数、メソッド、操作、コンストラクタに使い、`check_method_kinds` も使う)、`check_derived_fields` (check/mod.rs:336)、shape.rs の単体テストである
- ファイルは、check/mod.rs に足す `signature_file(program, function)` で決める
  - `FunctionKind::InstanceMethod(_, m)` と `DefaultMethod(m)` は `program.file(m.module)` にする。generics をクラスのメソッドから写しているので、`never` の範囲はクラスのファイルにある。`MethodId.module` はクラスのモジュールである
  - それ以外は `program.file(id.module)` にする。`declaration_schemes` のメソッドも `program.file(id.module)` を使う
  - 操作とコンストラクタは自分のファイルを渡す。HIR がそれらの generics に `never` がないことを保証するので、何も足されない
- `Provenance::Bound(Span)` を足す。doc コメントは「シグネチャに書いた row 変数の上限。破れるのは同じ SCC の参照を通したときだけで、その参照に複写した制約が報告する」とする。`origin()` も `reportable` も `None` を返す
- SCC の外の参照は今と同じである。残した `σ_e ≤ Never` を `copy_scheme` が複写する
- 同じ SCC の参照
  - `merge` は、メンバーごとに `own_bounds: Vec<(usize, Multiplicity, Span)>` を前もって求める。`problem.mult` のうち `Bound(span)` の由来を持つ制約から作り、`k` は下の変数の `problem.own.mult` の中の位置である
  - `equate` の形を `fn equate(&mut self, lin: &[KindVar], mult: &[KindVar], callee: usize, origin: &Provenance)` にする。今の変数どうしの等しさを足した後、`own_bounds[callee]` の各 `(k, c, span)` について、`(Var(mult[k]), Const(c))` を由来 `bound_origin(origin, BoundSource::Written(span))` で足す。呼ぶ所は `merge` の1か所である
  - `bound_origin(o, s)` は、`o` が `At{_, Passed(name)}` なら `At{span: o.span, reason: RowBound{name, source: s, cause: None}}` を作り、それ以外なら `o.clone()` を返す
  - 写すのは書いた上限だけである。同じ SCC のメンバーの推論した上限は自分の制約でなく、どこかのメンバーの本体にある、SCC の外の呼び出し先からの複写である。その複写はすでに `RowBound` の由来を持つ。SCC は Kind の解を1つ共有するので、破れるのはその複写で、内側の参照に報告される
- 不変条件 (`merge` の doc コメントと architecture.md に書く): 「`never` を書いたシグネチャの宣言を指す参照はどれも、参照を由来とする `σ_参照 ≤ Never` を1つ持つ。SCC の外の参照は残した制約の複写 (`copy_scheme`) で、SCC の中の参照 (自分自身、相互再帰、ローカル関数の中からの参照を含む) は `equate` の複写で持つ。`Provenance::Bound` の制約は報告しない」。本体のどこからのトップレベルの値への参照も (ローカル関数の中を含む)、囲む宣言の `Instance` になるので (body.rs の `path` から `instantiate`)、すべての参照がこれに当たる
- `solve_scc` は、多重度の束に `solve_with_causes` を使う。`solve` を呼ぶのは単体テスト (kind/solve.rs のテスト、table/tests.rs:83) だけなので、`#[cfg(test)]` にする

#### D-14: E2020 の報告

- `lib.rs` に `ROW_BOUND_EXCEEDED = ErrorCode(2020)` を足す
- `kind/mod.rs` に次の型を足し、`order_key` に新しい枝を足す
  - `KindReason::RowBound { name: String, source: BoundSource, cause: Option<BoundCause> }`
  - `enum BoundSource { Written(Span), Through { name: String, span: Span }, Unknown }`
  - `struct BoundCause { span: Span, what: Exceeding }` と `enum Exceeding { Effect(EffectId), Multiplicity(Multiplicity) }`
  - `KindReason::Performs(EffectId)`
- `bind_row` (table/row.rs) は、今の由来が `At(o)` なら、ラベルごとの `Const ≤ σ` を `At{span: o.span, reason: Performs(label.effect)}` で記録する。ほかの由来はそのまま保つ
  - これでほかの報告は変わらない。読まれるのは破れた制約の由来だけで (solve.rs:43、:50、:118 の `with_cause` は線形性だけを読む)、`Const ≤ Var` の制約は破れない (solve.rs:340-345)。持ち越しは自分の由来 (`Carry.origin`) を使い、`residual` は由来を捨てる
- `KindScheme.bound_sources: Vec<(KindVar, BoundSource)>` を足す。変数の順に並べ、`mult` に残した `(Var(k), Const(_))` ごとに1項目を持つ
  - メンバー自身の σ_k が `Bound(span)` の制約を直接持てば `Written(span)` にする
  - そうでなければ、D-16 の探索の結果の `Through` にする。T-3 では探索がまだないので `Unknown` にする
- `copy_scheme` は、スキームの `(Var(v), Const(_))` を由来 `At{span, Passed(name)}` で写すとき、`bound_origin(origin, bound_sources[v])` を使う。線形性の制約、変数どうしの制約、下限は今と同じく `Passed` で写す
- 原因 (`solve_scc`): 破れた多重度の制約で報告するもののうち、理由が `RowBound` のものは、`mult_causes[lower]` を読む
  - 同じファイルの `At{span, Performs(e)}` なら `Effect(e)`
  - 同じファイルのほかの `At` なら `Multiplicity(lower の値)`
  - それ以外なら `None`
- 表示は、`report.rs` の `linear_misuse` に `KindReason::RowBound{..} => row_bound_exceeded(program, origin)` の枝を足して行う

  | 部分 | 文 |
  |---|---|
  | message | the row passed to `{name}` may perform `{Effect}`, but `{name}` allows only `never` operations。`Multiplicity` なら「…may perform `{once\|multi}` operations, …」、原因がなければ「…may perform operations other than `never` ones, …」 |
  | primary (参照) | `{name}` is used here |
  | secondary (原因) | `{Effect}` is performed here。`Multiplicity` なら「`{multi}` operations may be performed here」 |
  | secondary (`Written`) | `{name}` bounds this row variable by `never`。呼び出し先の `never` を指し、ファイルは `signature_file` で決める |
  | secondary (`Through`、T-4) | the row is passed to `{inner}` here |
  | note | a row variable written `never e` admits only `Panic` and effects whose operations are all `never`; `IO` and the effects of `var` are not among them |

  - 1つの呼び出しに1件だけ出す。今の `origins.dedup()` がまとめる
  - `Unknown` には出どころの secondary を付けない
- E2011 (`check_method_kinds`) は、`Unentailed` の項目がすべて `Multiplicity` のとき、次の note と help を出す。メッセージは変えない
  - note: the signature of a method allows any effects for its row variables, and this definition passes a row where only `never` operations are allowed
  - help: write `never` before the row variable in the signature of `{name}` in the class

#### D-15: 計測はしない

- この段が変えるのは `eml_syntax`、`eml_hir`、`eml_extern`、`eml_types`、`tests/ui`、`docs` である。roadmap の「進め方」の 2 が挙げる `crates/eml_vm`、`eml_rt`、`eml_lir`、`eml_core_ir`、`std/`、`bench/` は変えないので、`bench/*.sh` も benchmarks.md の表の入れ替えもしない
- `EffectRow` を読むのは `eml_types` だけである。`crates/eml_vm/tests/bench.rs` の `RunStats` は `cargo nextest run` の中で確かめる

#### D-16: 推論した上限の出どころの探索 (`Through`)

- `fn through_source(merged, member, k, c) -> BoundSource` を kind/solve.rs に足す
- グラフは、`merged.mult` の変数どうしの制約の上向きの隣接 (`lower → upper`) で、`solve_scc` ごとに1回作る
- `own[member].mult[k]` から幅優先で探し、訪ねた変数の集合を持つ。`equate` が作る輪でも止まる
- 候補は、訪ねた変数 `x` の制約 `(Var(x), Const(c'))` のうち、`c' == c` で由来が `At{span, RowBound{name: inner, ..}}` のものである。それを `Through{inner, span}` にする。ほかの変数の `Bound` の由来は見ない。ほかのメンバーのもので、`equate` を通してだけ届き、その `RowBound` の複写が候補になるからである
- 候補が複数あれば、`(span.file, span.range.start, span.range.end, inner)` のいちばん小さいものを選ぶ
- 深さは1段で、`CarriedThrough` と同じである。いちばん近い呼び出し先と、このメンバーの中の参照を書き、その先はたどらない
  - 鎖 `a → b → helper` では、`b` の出どころは `Through{helper, b の中}`、`a` の出どころは `Through{b, a の中}` である。`a` の悪い呼び出しには「the row is passed to `b` here」が出る
  - SCC `w1 ↔ w2` で `w1` が `helper` を呼ぶとき、`w1` と `w2` のどちらの出どころも `Through{helper, w1 の中の範囲}` である。そのため、ラベルに主語を書かない
- 見つからなければ `Unknown` にする

#### D-17: 試しの出力 (dump)

- `ty.rs` に `KindConstraint::RowUpper { row: String, upper: Multiplicity }` を足し、`<e> <= Never` と表示する
- `dump.rs` の `kind_constraints` は、`scheme.mult` の `(Var, Const)` のうち、変数が名前を持つ row 変数のものを1つずつ出す
- 今の dump はどれも変わらない (試作で確かめた)

## 6. 実装の決まりと約束

- `docs/spec/` の規則は、その規則を実装するコミットに書く。どのタスクがどの文を書くかは 7 章のとおりで、実装より先に規則を書かない (D-1 から D-7)
- T-2 と T-3 のあいだは、`<never e>` を読めても Kind の意味を持たない。T-2 の spec は構文と書ける場所だけを述べるので、spec とコードは食い違わない。この状態は段の枝の中だけにある
- コードのコメントと `docs/` は日本語で書き、`yomiyasu:yomiyasu` のスキルに従う。コメントは理由を書き、規則は `docs/` の節を引く。計画の ID (D-x、T-x) と計画の文書は引かない
- 下の spec の文は計画の段階の案である。意味 (主張、条件、言い切りの強さ) を変えずに、yomiyasu で整えてから書く
- E1057 と E2020 のメッセージ、ラベル、help、note の英語の文は D-11 と D-14 のとおりにする
- 既存のテストの期待値は変えない。変わったら止めて報告する。種類 1 と 2 の変更はこの段にない。種類 3 は T-3 の shape.rs の単体テストだけである
- 新しいテストは、それぞれのタスクの中で先に書く (TDD)
- 各タスクの後で、`cargo clippy --all-targets -- -D warnings` が警告なしで終わり、`cargo fmt --check` が差分を出さず、`cargo nextest run` がすべて通ること

## 7. 実装のタスク

各タスクは1つのコミットで、それだけで確かめられる。

| ID | タスク | 基づく決定 | 依存 | 並行 |
|---|---|---|---|---|
| T-1 | `Panic` を `Never` のラベルにする | D-4、D-12 | - | T-2 と並行できる |
| T-2 | 構文、HIR、E1057 | D-1、D-2、D-7、D-10、D-11 | - | T-1 と並行できる |
| T-3 | 上限の制約と E2020 | D-2、D-3、D-5、D-7、D-13、D-14、D-17 | T-1、T-2 | できない |
| T-4 | 推論した上限の出どころ | D-16 | T-3 | T-5 と並行できる |
| T-5 | 上限から導ける持ち越しを残さない | D-6 | T-3 | T-4 と並行できる |
| T-6 | UI テスト | D-1 から D-7 | T-4、T-5 | できない |
| T-7 | `Close never-rows` | すべて | T-6 | できない |

T-1 と T-2、T-4 と T-5 は別のファイルを触るが、同じ枝に順に積む。並行に作るときは、後に入れるほうを先のほうの上に rebase する。

### T-1: `Panic` を `Never` のラベルにする

- 目的: extern のエフェクトの多重度を表の行で決め、`Panic` を `Never` にする (D-4、D-12)
- 対象: `crates/eml_extern/src/lib.rs`、`crates/eml_types/src/context.rs`、`crates/eml_types/src/table/tests.rs`
- 作業
  - 先にテスト `panic_is_never` を table/tests.rs の `an_extern_effect_is_performed_at_most_once` の隣に書く。`table.effect_multiplicity(table.context.externs.panic) == Multiplicity::Never` を確かめる
  - D-12 のとおりに `EffectMultiplicity` と `EffectRow.multiplicity` を足し、`Context::new` を直す
  - spec を書く
    - types.md の「Kind」の2つ目の箇条を、次の案にする

      > - `Row<s>` の `s` は、その row に含まれてよい操作の上限を表す。`Never` は `never` の操作だけ (`Panic` と、`never` の操作だけを持つエフェクト) を含んでよい。`Once` はさらに `once` の操作 (extern のエフェクトの `IO` と、`var` のエフェクトを含む) まで、`Multi` はさらに `multi` の操作まで含んでよい。row の多重度は、ラベルのエフェクトの多重度と末尾の row 変数の多重度の上限 (join) である

    - types.md の「Kind」の最後の箇条 (「extern のエフェクト (`IO` と `Panic`) の多重度は `Once` と決める …」) を、次の案にする

      > - extern のエフェクトの多重度は、extern の型の Kind と同じく処理系の表の行が決める。`IO` は `Once`、`Panic` は `Never` である。どちらの操作も `never` の操作 (`io_failed`、`panicked`) だけなので、操作から計算すればどちらも `Never` になる。表の決め方は次のとおりである。起こすと呼び出し元へ戻らず、外の世界に何も残さないエフェクトは `Never` にする。`Panic` がこれに当たる。extern の関数がその場で1回だけ戻るか、外の世界への書き込みが順序を持つエフェクトは `Once` にする。`IO` がこれに当たり、並列のタスクの row (`Row<Never>`) に入らない

    - effects.md の「組み込みの `IO`」の段落「extern のエフェクトは、型検査では…」を、次の案にする

      > extern のエフェクトは、型検査では普通のエフェクトと次の点が違う ([型と Kind](types.md) の「推論」)。row の中で重なったラベルは1つにまとめ、`mask` には入れない。多重度は処理系の表の行が決め、`IO` は `Once` である ([型と Kind](types.md) の「Kind」)。

    - effects.md の「組み込みの `Panic`」の箇条「型検査では `IO` と同じく扱う …」を、次の案にする

      > - 型検査では、重なったラベルを1つにまとめ、`mask` に入れない点が `IO` と同じである。多重度は `Never` で、`never` の操作だけを含む row (`Row<Never>`) に入れられる。並列のタスクに渡す関数も `Panic` を起こせる ([マルチコア対応の設計](../future/multicore.md))。操作は `never` の `panicked` だけなので、`Panic` は yield しうるラベルでない ([型と Kind](types.md) の「row の具体化の記録」)。row に `Panic` を足しても、yield の確かめは増えない

    - stdlib.md の `Panic` の宣言 (stdlib.md:86) の近くに、多重度は types.md の「Kind」にあることを1文で書く
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件
  - `cargo nextest run -p eml_types` → すべて通り、`panic_is_never` を含む
  - `cargo nextest run` → すべて通る
  - `grep -n "多重度は \`Once\` と決める" docs/spec/types.md` → 何も出ない
- 落とし穴: `EffectRow` を struct の式で作る所は `eml_extern` の表だけである。ほかに作る所が見つかったら、それも直す

### T-2: 構文、HIR、E1057

- 目的: `'never'? LIDENT` を読み、上限を HIR の row 変数に持たせ、書けない所を E1057 にする (D-1、D-2、D-7、D-10、D-11)
- 対象
  - eml_syntax: `src/grammar/types.rs`、`src/ast.rs` (`EffectRow::bound()`)
  - eml_hir: `src/hir.rs`、`src/lib.rs`、`src/lower/{types,effect,data}.rs`、`src/pretty.rs`
- 作業
  - 先に、`crates/eml_syntax/tests/types.rs` に次のテストを書く

    | テスト | 入力 | 期待 |
    |---|---|---|
    | `a_row_variable_can_be_bounded_by_never` | `h : Unit -> <never e> Unit` | `shape` の snapshot で、`EFFECT_ROW` が `L_ANGLE`、`NEVER_KW "never"`、`LIDENT "e"`、`R_ANGLE` を持つ |
    | `bounded_row_variables_in_every_row_position` | `x : Iter <never e> a`、`f : a -> <Panic \| never e> b`、`f : a -> <IO, Panic \| never e> b`、`f : (Unit -> <never e> a) -> <e> a` のそれぞれ | 診断なし |
    | `only_never_can_bound_a_row_variable` | `f : Unit -> <once e> Unit` と `f : Unit -> <multi e> Unit` | それぞれ `["E0011 1:14 only \`never\` can bound a row variable"]` |
    | `never_must_be_followed_by_a_row_variable` | `f : Unit -> <never Panic \| e> Unit` | `["E0011 1:20 expected a row variable"]` |
    | (同じテスト) | `f : Unit -> <never> Unit` | `["E0011 1:19 expected a row variable"]` |
    | `a_row_parameter_cannot_be_bounded` | `data T <never e> =\n  \| T (Unit -> <e> Int)` | `["E0011 1:9 a row parameter of \`data\` cannot have a bound"]` |

  - 先に、`crates/eml_hir/tests/lower.rs` に次のテストを書く

    | テスト | 入力 | 期待 |
    |---|---|---|
    | `a_bound_belongs_to_the_row_variable` | `f : (Unit -> <e> Int) -> <never e> Int\nf g = g ()` | `lower_text` の snapshot で、2つの出現のどちらも `<never e>` |
    | `an_annotation_may_repeat_a_declared_bound` | `helper2 : (Unit -> <never e> Int) -> <e> Int\nhelper2 f =\n  let g : Unit -> <never e> Int = f\n  g ()` | 診断なし |
    | `a_bound_must_be_declared_in_the_signature` | `helper3 : (Unit -> <e> Int) -> <e> Int\nhelper3 f =\n  let g : Unit -> <never e> Int = f\n  g ()` | `["E1057 3:20 \`never\` cannot bound \`e\` here"]` |
    | `a_data_field_cannot_bound_a_row_variable` | `data T <e> =\n  \| T (Unit -> <never e> Int)` | `["E1057 2:17 \`never\` cannot bound \`e\` here"]` |
    | `an_operation_cannot_bound_a_row_variable` | `effect E where\n  op : (Unit -> <never e> Int) -> Int` | `["E1057 2:18 \`never\` cannot bound \`e\` here"]` |
    | `a_bounded_row_parameter_stays_a_parameter` | `data T <never e> =\n  \| T (Unit -> <e> Int)` | `E0011 1:9 …` だけで、E1002 がない |

    列の位置は、この文字列で計算した値で、試作の出力と一致した。テストの補助関数の出力の形 (`"E0011 1:14 …"`) が既存のテストと違えば、既存の形に合わせる
  - D-10 と D-11 のとおりに実装する
  - spec を書く
    - grammar.md の `row` の規則を D-1 の EBNF にし、「文法上の補足」の `data` の型引数の箇条の後に、次の案を足す

      > - row の末尾の row 変数の前には、多重度の上限 `never` を書ける (`<never e>`、`<Panic | never e>`)。上限は row 変数に付くもので、row 全体には付かない。`never` の後ろが row 変数でない形 (`<never Panic | e>`、`<never>`) と、`never` 以外の多重度 (`<once e>`、`<multi e>`) は E0011 にし、row の残りは読んで回復する。`data` の型引数の `<e>` には上限を書けない (E0011)

    - types.md の「1回だけ呼べる関数の型」の直後に、新しい節「row 変数の多重度の上限」を作り、この段では次の箇条だけを書く

      > - シグネチャの row 変数の前に `never` を書ける。`once a -> …` が矢印の Kind を書くのと同じ並びである。上限に書けるのは `never` だけである
      > - 1つのシグネチャの同じ名前の row 変数は1つの変数である。上限は、その変数のどの出現に書いてもよく、書かなかった出現も同じ上限を持つ
      > - 上限を宣言できるのは、row 変数を量化するシグネチャ (トップレベルの関数と値、extern の関数、クラスのメソッド) だけである。本体の型の明示 `(e : T)`、`let` と `var` の型、ラムダの引数の型はシグネチャの変数を指すので、シグネチャが `never` を付けた変数にだけ `never` を書ける。付けていない変数に書いたら E1057 にする。`data` のフィールドの row 変数は宣言の型引数で、上限を持たないので、同じく E1057 である。操作のシグネチャにも書けない (E1057)。操作ごとの row 変数は節の中で `Multi` 以上なので ([エフェクトと handler](effects.md) の「handler の意味」)、上限と両立しない

    - declarations.md の操作の箇条に、「row 変数に `never` は書けない (E1057)」を足す
    - docs/spec/diagnostics.md の表の E1056 の後に、次の行を足す

      > | E1057 | `ROW_BOUND_NOT_DECLARED` | row 変数の上限 `never` を、シグネチャが付けていない変数の本体の型の明示か `data` のフィールドに書いた、または操作のシグネチャに書いた ([型と Kind](types.md) の「row 変数の多重度の上限」) |

    - docs/implementation/diagnostics.md の表の E1056 の後に、E1057 の行を足す。見出し、範囲、場面ごとのラベルと help は D-11 の表のとおりで、secondary を付けないことも書く
- 実装者に任せること: テストの補助関数の名前 (`shape`、`lower_text` など) は既存のテストに合わせる
- 条件: いつも
- 完了の条件
  - `cargo nextest run -p eml_syntax` → すべて通る
  - `cargo nextest run -p eml_hir` → すべて通る
  - `cargo clippy -p eml_test_support --no-default-features --features hir -- -D warnings` → 警告なしで終わる
  - `cargo nextest run` → すべて通る
- 落とし穴
  - `never` の後ろに LIDENT がない形を回復しないと、`Panic` に E1002、`|` に「expected `)`」が続けて出る
  - `once` を row の頭で読み飛ばさないと、関数型の `once` として読まれ、「expected `->`」と E2001 が続く
  - `row_param` でキーワードを読み飛ばさないと、`e` を使うフィールドがすべて E1002 になる

### T-3: 上限の制約と E2020

- 目的: 上限を Kind のスキームの制約にし、超える呼び出しを E2020 にする (D-2、D-3、D-5、D-7、D-13、D-14、D-17)
- 対象
  - `shape.rs` (`instantiate_rigid` の `file` の引数)
  - `table/{mod,row}.rs` (`mult_at_most`、`Performs`)
  - `check/mod.rs` (`signature_file`、呼び出す所、`declaration_problem` の引数、E2011 の note)
  - `kind/{mod,problem,solve}.rs` (`Provenance::Bound`、`RowBound`、`BoundSource`、`BoundCause`、`Written | Unknown` の `bound_sources`、`equate` の複写、`copy_scheme`、原因、`#[cfg(test)]` の `solve`)
  - `check/report.rs` (`row_bound_exceeded`)
  - `lib.rs` (E2020)
  - `ty.rs` と `dump.rs` (`RowUpper`)
- 作業
  - 先に、新しいファイル `crates/eml_types/tests/row_bounds.rs` を作り、`tests/main.rs` で宣言する。共通の頭に次の2つを置く

    ```haskell
    helper : (Unit -> <never e> Int) -> <e> Int
    helper f = f ()

    noisy : Unit -> <IO> Int
    noisy () =
      println "x"
      1
    ```

    `noisy` の本体の書き方は今の構文に合わせてよいが、`a_self_call_exceeds_its_own_bound` の行と列はこの頭の行数で決まるので、頭を変えたら期待の位置も計算し直す
  - Kind の dump のテスト

    | テスト | プログラム | 期待 |
    |---|---|---|
    | `a_written_bound_stays_in_the_scheme` | `helper` | `kinds(…, "helper") == "  kinds: <e> <= Never"` |
    | `a_call_infers_the_bound` | `wrap : (Unit -> <e> Int) -> <e> Int` と `wrap f = helper f` | `wrap` も同じ行 |
    | `a_recursive_group_shares_an_inferred_bound` | `w1` と `w2` が互いを呼び、`w1` が `helper` を呼ぶ | どちらも同じ行 |
    | `an_unused_row_variable_has_no_bound` | `free : (Unit -> <e> Int) -> <e> Int` と `free f = f ()` | `""` |

  - 受け付けるプログラム (`check_short == []`)
    - `panic_fits_a_never_bound`: `<IO, Panic>` の `main` で `helper (fn () -> 1 / 0)` を呼ぶ
    - `an_effect_of_never_operations_fits_a_never_bound`: `import Fail (Fail(..))` で、`run : Unit -> <Fail String> Int` の本体が `helper (fn () -> fail "no")` を呼ぶ
    - `labels_beside_a_bounded_variable_are_not_bounded`: `big : (Unit -> <never e> Int) -> <IO | never e> Int` と、`tab : Int -> (Int -> <never e> Int) -> <Panic | e> List Int`
  - E2020 のテスト (`check_full` の snapshot)

    | テスト | プログラム | 期待 |
    |---|---|---|
    | `a_call_that_exceeds_a_written_bound` | `main () = println (show (helper noisy))` | `helper` に E2020。原因「`IO` is performed here」が `noisy` に、`Written` の secondary が `helper` のシグネチャの `never` に付き、note が出る |
    | `a_multi_operation_exceeds_a_never_bound` | `effect Choose where\n  multi choose : Unit -> Bool` と `pick : Unit -> <Choose> Int`、`pick () = helper (fn () -> if choose () then 1 else 2)` | message が「…may perform `Choose`…」で、原因が `choose ()` に付く |
    | `unwritten_occurrences_carry_the_bound` | `pair : (Unit -> <never e> Int) -> (Unit -> <e> Int) -> <e> Int` を `pair (fn () -> 1) noisy` と呼ぶ | `pair` に E2020、原因が `noisy` |
    | `a_self_call_exceeds_its_own_bound` | `h : (Unit -> <never e> Int) -> <IO \| e> Int\nh f = h noisy` | E2020 がちょうど1件で、内側の `h` (頭の5行と空行の後の 7:7) を指す。原因が `noisy`、`Written` が 6:15 |
    | `a_mutual_call_exceeds_the_bound` | `m1 : (Unit -> <never e> Int) -> Bool -> <IO \| e> Int\nm1 f b = if b then f () else m2 b\n\nm2 : Bool -> <IO> Int\nm2 b = m1 noisy b` | E2020 がちょうど1件で、`m2` の中の `m1` を指す |
    | `a_call_from_a_local_function_exceeds_the_bound` | `h : (Unit -> <never e> Int) -> <IO \| e> Int\nh f =\n  let go x = h noisy\n  go 1` | E2020 がちょうど1件で、`go` の中の `h` を指す |
    | `a_method_call_exceeds_the_class_bound` | `class Runs a where\n  run : a -> (Unit -> <never e> Int) -> <e> Int` の `run` に `noisy` を渡す | `run` に E2020。extern と同じ `declaration_problem` の道を通る |

  - `crates/eml_types/tests/classes.rs` のテスト
    - `a_method_can_rely_on_a_bound_in_the_class_signature`: 上の `Runs` に `instance Runs Int where\n  run n f = helper f + n` を置き、結果の row を `<Panic | e>` にした形で、診断なし
    - `a_method_that_needs_a_bound_the_class_lacks`: クラスのシグネチャを `<e>` にした同じ instance で、E2011 に D-14 の note と help が付くことを `check_full` の snapshot で確かめる
  - D-13、D-14、D-17 のとおりに実装する
  - spec を書く
    - types.md の「Kind」の箇条「内部では Kind 変数と部分 Kind 関係を扱う。…3つである」を、次の案にする

      > - 内部では Kind 変数と部分 Kind 関係を扱う。表面の構文で書ける Kind は、1回だけ呼べる関数の型 `once`、row 変数の多重度の上限 `never`、`data` の row の型引数、`data` の `Lin` の宣言の4つである (下の各節)。それ以外の Kind は書かせない。

    - types.md の「Kind」の箇条「`Never` は、並列タスクの row を … 内部の束に置く …」を、次の案にする

      > - `Never` は、`never` の操作だけを含む row の Kind である。並列のタスクに渡す関数の row をこれに制限するために使う ([マルチコア対応の設計](../future/multicore.md))。束は `Never ≤ Once ≤ Multi` の3要素である

    - 節「row 変数の多重度の上限」の頭に例を置き、T-2 の箇条に次の意味の文を足す。1つ目の箇条は、T-2 の「シグネチャの row 変数の前に `never` を書ける」の箇条を置き換える

      ```haskell
      par     : (Unit -> <never e> a) -> (Unit -> <never e> b) -> <e> (a, b)
      compute : (Unit -> <never e> a) -> <IO, Async | e> a
      ```

      > - シグネチャの row 変数の前に `never` を書くと、その row 変数の Kind `Row<σ>` に上限の制約 `σ ≤ Never` が付く。`once a -> …` が矢印の Kind を書くのと同じ並びで、`never` は操作の多重度の `never` と同じ意味を表す。上限に書けるのは `never` だけである。`Once` の上限は使う所がなく、`Multi` は束の最大元で何も制限しない
      > - 上限は row 変数に付く。ラベル付きの row の末尾に書くときも変数の前に書く (`<Panic | never e>`)。row のラベルは上限に関わらない。`<IO | never e>` は `IO` を含む row で、`e` だけが `Never` に制限される
      > - (T-2 の「1つのシグネチャの同じ名前の row 変数は1つの変数である …」の箇条の後ろに足す) `par` の結果の row `<e>` も `Never` である。子の row だけを制限し、続きの関数の row を制限しないときは、別の row 変数を使う (`(Unit -> <never e1> a) -> (a -> <e> b) -> <e> b`)。子の `never` の操作を結果の row に流すには、`<Panic | e>` のようにラベルで書く
      > - 上限を持つ row 変数が結果の row にあっても、呼び出す側の row は制限されない。呼び出しの包含は呼び出し先の末尾を今の row と単一化せず、末尾の変数の多重度を今の row の多重度に引き上げないためである (下の「呼び出しの包含を解く」)。`<IO, Panic>` の本体からも `par` を呼べる
      > - 上限は宣言の Kind のスキームの制約である (下の「推論」)。上限を超える多重度の row を、その row 変数に入れる呼び出しは E2020 にする
      > - 上限は表示しない。REPL の `:type` と診断の型の表示は、上限を書いたシグネチャでも `<e>` と書く (下の「Kind の書き出し」)

    - types.md の「推論」の「Kind はシグネチャから決まらないので推論する」の下 (`dup` の例の後) に、次の案の下位の箇条を足す

      >   - シグネチャに書いた row 変数の多重度の上限 (上の「row 変数の多重度の上限」) は、本体を検査する前に、その宣言の Kind の制約に足す。本体が求めなくてもスキームに残り、呼び出すたびに複写される。書かなかった上限は本体から推論する。上限を持つ row 変数に引数の row を入れる呼び出しは、その row 変数の多重度を引数の多重度以上にするので、`par` に渡す関数を受け取る関数は、書かなくても同じ上限をスキームに持つ。上限の違反はいつも呼び出しで起きる。ラムダ、handler の節、ローカル関数、今の row のどれから来た row でも、上限を持つ row 変数に入れた呼び出しを指して E2020 にする。自分の本体の中で上限を破るのは、自分を (同じ SCC の関数を通して) 呼ぶ呼び出しだけで、それもほかの呼び出しと同じく E2020 にする

    - types.md の「書き出す型の形」の「多重度 (`once` / `multi`) は書き出さない。」を、次の案にする

      > 多重度は書き出さない。row 変数の多重度の上限 (`<never e>`) も、書いたものも推論したものも書き出さず、型の表示は `<e>` と書く。後の段階は多重度を読まず、Kind の制約の表示は `once` の矢印だけに限るためである。上限は、それを超えた呼び出しの診断 (E2020) が伝える。REPL はセッション全体を入力ごとに検査し直すので、上限を入力の間で持ち越す必要はない。

    - types.md の「instance と既定のメソッドの検査」の箇条「導けない制約 …」の後ろに、次の案を足す

      > シグネチャの row 変数に多重度の上限 (`<never e>`) を書けば、その上限を前提にできる。instance が `e` の関数を `par` のような `Row<Never>` を求める位置に渡すときは、クラスのシグネチャに `<never e>` が要る

    - grammar.md の T-2 の箇条の最後に、「上限の意味と書ける場所は [型と Kind](types.md) の「row 変数の多重度の上限」で定める」を足す
    - overview.md:31 の「`Never` はマルチコア対応のための要素」を、「`Never` は `never` の操作だけの row の Kind で、シグネチャに `<never e>` と書ける」にする
    - multicore.md の「row の Kind」の最後の箇条 (「今の実装は内部の Kind の束を3要素で用意する …」) を、「`Panic` の多重度と `<never e>` の規則は [型と Kind](../spec/types.md) にある」にする
    - docs/spec/diagnostics.md の表の E2019 の後に、次の行を足す

      > | E2020 | `ROW_BOUND_EXCEEDED` | 呼び出しが、多重度の上限を持つ row 変数に、上限を超える row を入れた。上限はシグネチャに書いたものと、推論で付いたものの両方を含む ([型と Kind](types.md) の「row 変数の多重度の上限」) |

    - docs/implementation/diagnostics.md の表の E2019 の後に E2020 の行を足す。D-14 の表の文のうち `Through` を除いたものを書く。E2011 の行に、多重度だけが導けないときの note と help を書く
    - architecture.md の型検査の節 (:236 付近、:242 付近、:266 付近) に、自分の上限が `instantiate_rigid` で入ること、`Provenance::Bound`、D-13 の不変条件を書く
- 実装者に任せること
  - 新しい型と関数を置くファイルの中の位置
  - `bind_row` で由来を `Performs` にする書き方 (由来が `At` のときだけ変え、ほかは保つことは変えない)
- 条件: いつも
- この時点の振る舞い (spec と食い違わない)
  - 推論した上限の違反には、出どころの secondary が付かない (`Unknown`)
  - `Lin` の値を持つ、上限を書いた関数の悪い呼び出しには、E2020 と E3006 が両方出うる
- テストの変更の種類: 種類 3 だけで、`instantiate_rigid` を呼ぶ shape.rs の単体テストに引数を足す。上限を書かないので、期待値はバイト単位で変わらない
- 完了の条件
  - `cargo nextest run -p eml_types --test integration row_bounds::` → すべて通る
  - `cargo nextest run -p eml_types --test integration -E 'test(classes::)'` → すべて通る
  - `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` → 警告なしで終わる
  - `cargo clippy -p eml_cli --no-default-features --features core -- -D warnings` → 警告なしで終わる
  - `cargo nextest run` → すべて通り、既存のテストの期待値の差分がない (`cargo insta pending-snapshots` が何も出さない)
- 落とし穴
  - 同じ SCC の参照で自分の上限に `Declaration` の由来を付けると、`reportable` の `unreachable!` で落ちる。`Given` にすると違反が報告されない。`Provenance::Bound` と `equate` の複写の両方が要る (D-13)
  - 上限を `Shape::instantiate` でも足すと、SCC をまたぐ呼び出しが2件ずつ報告される
  - `declaration_problem` に `file` を渡し忘れると、`Written` の secondary が違うファイルを指す。instance と既定のメソッドはクラスのファイルである

### T-4: 推論した上限の出どころ

- 目的: 推論した上限の違反に、いちばん近い呼び出し先を secondary として付ける (D-16)
- 対象: `kind/solve.rs` (`through_source`)、`bound_sources` を作る所
- 作業
  - 先に、row_bounds.rs に次のテストを書く (`check_full`)

    | テスト | プログラム | 期待 |
    |---|---|---|
    | `an_inferred_bound_names_the_inner_call` | `wrap` (T-3 と同じ) を `main` が `wrap noisy` と呼ぶ | `wrap` に E2020。secondary「the row is passed to `helper` here」が `wrap` の中の `helper` に付く |
    | `a_chain_names_only_the_nearest_call` | `b f = helper f`、`a f = b f`、`main` が `a noisy` を呼ぶ | `a` に E2020。secondary「the row is passed to `b` here」が `a` の中の `b` に付く |
    | `a_recursive_group_names_the_call_in_the_group` | `w1` と `w2` の輪で、`main` が `w2 noisy True` を呼ぶ | `w2` に E2020。secondary「the row is passed to `helper` here」が `w1` の中の `helper` に付く |
    | `an_inferred_bound_broken_inside_its_group` | `w1 : (Unit -> <e> Int) -> Bool -> <IO \| e> Int`、`w1 f b = if b then w2 b else helper f`、`w2 : Bool -> <IO> Int`、`w2 b = w1 noisy b` | E2020 が1件で `w1` の中の `helper` を指し、原因が `w2` の中の `noisy`、`Written` の secondary が付く |

  - D-16 のとおりに実装する。T-4 の後は、`Passed` の複写が `Unknown` にならないことを `debug_assert!` で確かめる
  - docs/implementation/diagnostics.md の E2020 の行に、`Through` の secondary の文を足す
- 実装者に任せること: なし
- 条件: いつも
- テストの変更の種類: なし。T-3 は推論した上限の違反の snapshot を持たない
- 完了の条件
  - `cargo nextest run -p eml_types` → すべて通る
  - `cargo nextest run` → すべて通る
- 落とし穴: `equate` が両向きの辺を作るので、訪ねた変数の集合がないと止まらない

### T-5: 上限から導ける持ち越しを残さない

- 目的: 1つの悪い呼び出しが E2020 と E3006 の両方になるのを防ぐ (D-6)
- 対象: `kind/solve.rs` (`carry_residual`)
- 作業
  - 先に、row_bounds.rs に `a_bound_drops_the_carry_it_entails` を書く
    - プログラム: `hold : (Unit -> <never e> Unit) -> <IO | e> Unit` が `FileIn` を `action ()` の前後で持ち続ける。`main` は `handle hold pick` を動かし、`pick` は `multi choose` を起こす
    - 期待: `check_short` が E2020 を1件だけ出し、E3006 を出さない。`kinds(…, "hold") == "  kinds: <e> <= Never"` (`carry(Lin, <e>)` がない)
  - `carry_residual` で、row の側の自分の変数の残した上限 (`graph.reach(component, kept, true).1`) が `Once` 以下なら、その持ち越しを落とす
  - types.md の「推論」に、T-3 の下位の箇条の次に、次の案を足す

    >   - 上限が `Never` の row 変数を row の側に持つ持ち越しの制約は、上限から導けるので、多相化するときに残さない。残すと、上限を超える row を入れた呼び出しが E2020 と E3006 の両方になる

- 実装者に任せること: `hold` の書き方 (`FileIn` を開く所と、持ち続ける形)。E3006 が出る形であることを、落とす前の実装で一度確かめる
- 条件: いつも
- テストの変更の種類: なし。今の row 変数に定数の上限を持つものはないので、既存の dump は変わらない
- 完了の条件
  - `cargo nextest run -p eml_types` → すべて通る
  - `cargo nextest run` → すべて通る

### T-6: UI テスト

- 目的: 利用者に見える代表の出力を固める
- 対象

  | ファイル | 中身 |
  |---|---|
  | `tests/ui/run/kinds/never_row_bound.em` | `helper` に `1 / 0` を渡して `try_panic` で受け、`Fail.fail` を渡して `Fail.to_result` で受け、両方の結果を表示する |
  | `tests/ui/check-fail/types/row_bound_exceeded.em` | `helper noisy` を呼ぶ (T-3 の `a_call_that_exceeds_a_written_bound` と同じ形) |
  | `tests/ui/check-fail/names/row_bound_not_declared.em` | `data` のフィールドと本体の注釈 (E1057 が2件) |
  | `tests/ui/check-fail/names/row_bound_in_operation.em` | 操作のシグネチャ (E1057) |
  | `tests/ui/check-fail/syntax/row_bound_syntax.em` | `once_bound : (Unit -> <once e> Int) -> <e> Int` と `bound_label : (Unit -> <never Panic \| e> Int) -> <Panic \| e> Int` |

- 作業: ファイルを足し、`cargo insta review` で snapshot を受け入れる。受け入れる前に、各ファイルの診断が上の表の数と番号だけであることを目で確かめる
- 実装者に任せること: `run` のテストの表示の文と、`tests/ui/run/kinds/` がなければ近い既存の分類のディレクトリを選ぶこと
- 条件: いつも
- テストの変更の種類: なし (新しいテストだけ)
- 完了の条件
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → すべて通り、`run` のテストは検査モードと速いモードの両方で通る
  - `cargo nextest run` → すべて通る

### T-7: `Close never-rows`

- 目的: roadmap の「進め方」の閉じる作業を、1つのコミット `Close never-rows` で行う
- 作業 (「進め方」の順)
  1. spec を仕上げる。`grep -rn "内部の束に置く\|3つである" docs/spec` が何も出さず、段の名前と「今は〜、後で〜」の予告が `docs/spec` に残っていないことを確かめる
  2. 計測はしない (D-15)。コミットのメッセージに理由を書く
  3. docs/implementation/status.md の「今の言語の範囲」で、`once` の行 (:20 付近) に「row 変数の多重度の上限 `<never e>` (シグネチャで宣言し、本体の注釈でくり返せる。超える呼び出しは E2020。[型と Kind](../spec/types.md) の「row 変数の多重度の上限」)」を足し、extern の行 (:25 付近) に `Panic` の多重度が `Never` であることを足す
  4. architecture.md は、crate とパスの構成が変わらないので、T-3 の追記のほかは直さない
  5. roadmap.md を直す
     - `never-rows` の項目 (:30) を消す
     - `par-core` の「前提: `never-rows`」を「前提: なし」にする。後に回した項目のどれも `par-core` の前提でないためである
     - `par-core` の項目に「`par` の子に `IO` を起こす関数を渡すと E2020 になることを確かめる。`<never e>` を書いた extern の最初の利用である」を足す (10 章の Q-1)
     - 「その後の項目」の「型システム」に、次の4つを「後に回した理由: …。」と「見直す条件: …。」の入れ子の箇条を添えて足す
       - `<once e>`: 後に回した理由は、使う段がなく、`Tail` を束に挿入するかが決まっていないこと。見直す条件は、`Once` の上限を使うシグネチャが要る段ができたとき
       - `data` の row の型引数の上限 (`data Task <never e> a`): 後に回した理由は、型の適用ごとに制約を確かめる仕組みが要り、`par` には要らないこと。見直す条件は、上限を持つ row の型引数の `data` を使う段ができたとき
       - 操作の row 変数の上限: 後に回した理由は、操作ごとの row 変数が節の中で `Multi` 以上になり、上限と両立しないこと。見直す条件は、上限が節の規則 (`Multi` 以上) を置き換える設計を考えるとき
       - `:type` と診断での上限の表示: 後に回した理由は、表示のためだけに多重度の書き出しが要り、違反は E2020 が伝えること。見直す条件は、Kind の宣言化で Kind を一般に表示するとき
  6. overview.md は T-3 で直した。docs/README.md の「文書の地図」と CLAUDE.md は、規則も道しるべも変わらないので直さない
  7. docs/plans/2026-10-24-never-rows.md を消す。消す前に、5 章の決定がすべて 5 章の「リポジトリに残す所」に書かれていることを確かめる
  8. コミットのメッセージに、種類 1 と 2 のテストの変更がないこと、種類 3 が shape.rs の単体テストであること、足したテスト、roadmap で移した項目、計測しない理由、後に回した項目を決めた日 (2026-10-24) を書く
- 実装者に任せること: 「その後の項目」に足す4つの文の言い回し (理由と条件の中身は上のとおり)
- 条件: いつも
- 完了の条件
  - `cargo build` → 終了コード 0
  - `cargo nextest run` → すべて通る
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 警告なしで、差分もない
  - `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` と、`core` の同じコマンド → 警告なしで終わる
  - `cargo clippy -p eml_test_support --no-default-features --features hir -- -D warnings` → 警告なしで終わる
  - `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` → 警告なしで終わる
  - `test -e docs/plans/2026-10-24-never-rows.md || echo removed` → `removed`
  - `grep -n "never-rows" docs/future/roadmap.md` → 何も出ない

## 8. 実行の順

```
T-1 ─┐
     ├─ T-3 ─┬─ T-4 ─┐
T-2 ─┘       └─ T-5 ─┴─ T-6 ─ T-7
```

- T-1 と T-2 は並行に作れる。T-4 と T-5 も並行に作れる
- 枝には T-1、T-2、T-3、T-4、T-5、T-6、T-7 の順に積む

## 9. 検証の計画

- 基準 (最初の変更の前): `cargo nextest run` → 基点ですべて通る (計画の時点で 3512 passed、18 skipped を確かめた)
- 全体の検証 (すべてのタスクの後): T-7 の完了の条件のコマンドがすべて通る。加えて次を確かめる
  - `cargo nextest run -p eml_types --test integration row_bounds::` → すべて通る
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → すべて通る
  - 3 章の `par` と `compute` のシグネチャを書いた小さなファイルを試しのディレクトリに作り、`timeout 20 cargo run -p eml_cli -- check <file.em>` → 構文の誤りが出ない (本体は `f ()` などで埋める)
- 利用者が手で確かめること: なし

## 10. 危険と決めていない項目

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | extern の参照の道で、書いた上限がスキームに残ることをこの段では確かめない。`<never e>` を書く extern は std にしか書けず、表の行も要るためである。同じ `declaration_problem` の道を、クラスのメソッド (`a_method_call_exceeds_the_class_bound`) で確かめる | `par-core` の最初の extern | T-7 で roadmap の `par-core` に確かめることを書き、`par-core` が閉じる | `par-core` で確かめる |
| Q-2 | 操作の引数の row を使う handler の節の中で `helper` を呼ぶと、E2020 と E2002 が両方出る | 診断の数 | 実装者が出力を見て判断する | 両方を残す |
| Q-3 | T-4 の後、`Passed` の複写の出どころが `Unknown` になる場合 | secondary の欠け | `debug_assert!` で見つける | 見つかったら止めて報告する |

## 付録: 検討で変わったこと

蒸し返さないために、退けた案と変えた点だけを書く。

- E1051 と E1052 は使われている → E1057 と E2020 にした
- 上限を `once` と `multi` にも広げる案 → `never` だけにした (D-1)
- 本体の注釈で上限を一切書かせない案 → シグネチャが宣言した上限だけをくり返せる形にした (D-2)
- 「自分の本体の中で自分の上限は破れない」とした初めの案 → 同じ SCC の再帰で破れることを試作で確かめ、`Provenance::Bound` と `equate` の複写で参照に報告する形にした (D-3、D-13)
- `Panic` だけを名前で `Never` にする案 → `eml_extern` の表の列にした (D-4、D-12)
- 上限から導ける持ち越しを残す案 → 残さない (D-6)
- E1057 に、シグネチャの `e` を指す secondary を付ける案 (C-3) → 付けない。E1057 はちょうど `never` を書いていない場合なので、指す範囲が保存されていない。instance の本体ではクラスのファイルを HIR の下ろしが知らないためでもある
- REPL で上限を入力の間で持ち越す仕組み → 要らない。セッション全体を入力ごとに検査し直す (D-5)
