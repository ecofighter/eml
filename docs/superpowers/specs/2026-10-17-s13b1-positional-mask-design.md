# S13b-1 位置つきの `mask` の設計

## 目的と範囲

S13b (std のエフェクトと `Iter`) の1つ目の作業である。呼び出しの `mask` を「先頭から n 個を残し、その後ろの m 個を飛ばす」形に広げ、E2008 をなくす。

ねらいは、型を変換する handler を書けるようにすることである。次の `render` は、今は E2008 になる (2026-10-17 に試した)。

```haskell
effect Yield a where
  yield : a -> Unit

render : (Unit -> <Yield Int | e> Unit) -> <Yield String | e> Unit
render gen =
  handle gen () with
    | yield n k ->
      yield (show n)
      k ()
```

`handle` の本体の row は `<Yield Int, Yield String | e>` である。`gen` の row は `<Yield Int | e>` なので、`gen` 自身の `Yield` は1つ目の handler (`render` の handler) に届き、`e` を通る `Yield` は1つ目と2つ目の両方を飛ばして外へ届かなければならない。今の `mask` は呼び出しの中の `Yield` の操作をすべて同じ数だけ飛ばすので、両方を同時には満たせない。

ロードマップの S13b の「決めたこと: 位置つきの `mask` (S13b-1)」(`docs/future/roadmap.md`) を、この spec が細部まで決める。完了の条件は「`Yield Int` を受けて `Yield String` を出す handler が動く」ことである。std の `Yield` は S13b-3 で入れるので、この作業ではテストの中で宣言した `Yield` で確かめる。

次のものは範囲に入れない。

- `k` を書かない節 (`| fn op 引数 -> 本体`) と、ローカル関数どうしの相互再帰: S13b-2 で入れる
- std の `Yield`、`Fail`、`State`、`Std.Iter`、`Fs.read_line`: S13b-3 で入れる
- 同じエフェクトの2つ目のラベルを名指す構文 (Koka の `mask<yield>{…}`): ロードマップの「言語機能と構文」に置いたまま
- `mask` の計算の Verus での証明: [検証](../../future/verification.md) が優先しないと決めたまま

決めたことは、段の終わりに下の「段の終わりの文書」の場所へ移す。

## 利用者から見た動き

- 今 E2008 になるプログラムが、型検査を通って動く。呼び出し先が自分で起こすエフェクトは、呼び出し先の row の同じエフェクトのラベルと対になった handler に届き、row 変数を通るエフェクトは、その間にある handler を飛ばして外側に届く
- 今通るプログラムの意味は変わらない。今の `mask` は、新しい `mask` の n が 0 の場合に当たるためである
- E2008 は欠番になる。ほかの診断には使わない
- `mask` は今と同じく表面の構文に書かせない

## 規則

### `mask` の形

`mask` は、エフェクトごとの項目の集まりである。項目は (エフェクト `L`、残す数 n、飛ばす数 m) で、m は 1 以上、n は 0 以上である。1つの `mask` は、同じエフェクトの項目を2つ持たない。

`mask` 付きの呼び出しの間に起きた `L` の操作が handler を探すときは、その呼び出しより外側にある `L` の handler のうち、内側から数えて n 個を残し、続く m 個を飛ばす。呼び出しの中で設けた handler は、今と同じく飛ばさない。項目のないエフェクトの handler は飛ばさない。

### 型検査での決め方

呼び出し先の矢印の row を `<ls | e>` とし、`e` が rigid な row 変数のときを考える。`ls` のラベルは、今と同じく、今の row の同じエフェクトのラベルと先頭から対になる。対にならずに `e` の手前に余った今の row のラベルを rest と呼ぶ。

rest に現れる handle できるエフェクト `L` ごとに、次の項目を作る。

- n: `ls` の中の `L` の数
- m: rest の中の `L` の数

scoped labels の対にし方から、今の row の `L` の並びは `[ls と対になる n 個, rest の m 個, e に当たる分]` である。m 個を飛ばすと、呼び出し先から見える `L` の並びは `[n 個, e に当たる分]` になり、呼び出し先の row の `L` の並びと一致する。

末尾が推論用の変数のときと、閉じた row のときは、今と同じく `mask` を持たない。handle できないラベル (extern のエフェクト) は、今と同じく `mask` に入れない。

`k` の呼び出しも普通の呼び出しなので、`k` の最後の矢印の row (handle の外側の row) を今の row に含めるときに、同じ規則で `mask` を作る。

例を次の表に示す。

| 今の row | 呼び出し先の row | `mask` |
|---|---|---|
| `<State Int \| e>` | `<e>` | `State` を n = 0、m = 1 |
| `<State Int, State Int \| e>` | `<e>` | `State` を n = 0、m = 2 |
| `<Yield Int, Yield String \| e>` | `<Yield Int \| e>` | `Yield` を n = 1、m = 1 |
| `<Log, Log \| e>` | `<Log \| e>` | `Log` を n = 1、m = 1 |
| `<Log, State Int \| e>` | `<Log \| e>` | `State` を n = 0、m = 1 |

### 健全性

[エフェクトと handler](../../spec/effects.md) の「健全性」を、次のように書き直す。健全性の敵対的レビュー (2026-10-17) では反例は見つからなかったが、今の不変条件の言い方では帰納法が回らない所があると指摘された。それを直す。

- 不変条件の row を、動いている instance (またはクロージャ) の具体化で rigid な row 変数を置き換えた row にする。この row は具体的な row で、末尾は `main` の `<IO>` に行き着く。不変条件は「どのエフェクトについても、見える handler の並びは、その row の同じエフェクトの handle できるラベルの並びで始まる」である。今の言い方は静的な row のラベルだけを述べるので、rigid な末尾 `e` の手前のラベルの後ろに何が見えるかを述べていない。呼び出しの項は「m 個を飛ばすと、残りが `e` の具体化に当たる」ことを使うので、それを不変条件が含む必要がある。今の「今の row の `e` に当たる handler」という言い方は、この形で定まる
- 「一致する」でなく「で始まる」(先頭の部分の一致) と書く。閉じた row の呼び出し先とラムダでは、外側の handler が見えたまま残るためである
- 呼び出しの項: `ls` に現れるかどうかでエフェクトを分ける今の議論を、エフェクト `L` ごとの n と m の議論に置き換える。`e` の手前にある今の row の `L` は、先頭の n 個が `ls` と対になり、残りの m 個が rest にある。`mask` の項目が m 個を飛ばすので、呼び出し先の中で見える `L` の handler の並びは、呼び出し先の row (`e` を具体化したもの) の `L` のラベルの並びで始まる。`L` が `ls` に現れなければ n = 0 で、今の議論と同じになる
- 「`mask` の各項目は、その呼び出しの時点で見える別々の handler に対応する」「`mask` の包みが残すフレームの数は handler の数以下に収まる」の段落は、項目が飛ばす m 個の handler について同じく成り立つ。言い回しを「項目が飛ばす handler」に合わせる
- [Core IR](../../spec/core-ir.md) の「`perform` と節の3つの形」の、その場で動く節の議論に1文を足す。その場で動く節 g は `perform` の場所の `evv` のまま動くので、g の中の `mask` が残す n 個と飛ばす m 個は、g が自分で設けた handler だけでなければならない。g が `yields` でなければ、その instance の外側の row はラベルを持たない。g が `yields` で `handle` か `mask` 付きの呼び出しを持てば、それは yield の地点なので g はその場で動かない (`evidence/handlers.rs` が両方を yield の地点に数える)。そのため、`e` の手前のラベルはすべて節が設けた handler で、`e` を通る操作もない

### 推論用の row 変数が末尾にある場合

n が正の `mask` が付くのは、呼び出し先の末尾が rigid な row 変数のときだけである。たとえば、注釈のないラムダの中の handle で、外側の row が `<L | ρ>` (`ρ` は推論用の変数) のとき、節の中の `handle (k x) with …` (`L` の handler) の `k` の呼び出しは、`<L | ρ>` を `<L, L | ρ>` に含める単一化になり、エフェクトが足りない誤りになる。同じプログラムに rigid な末尾のシグネチャを付けると、`L@1` の `mask` で通る。今もこの形は誤りなので、後退ではない。[型と Kind](../../spec/types.md) の「推論」の、ラムダの row が呼び出しの順で決まることを述べた所に、この場合も書き足す

## 設計

### 型検査 (`eml_types`)

- `Table::include_row` の `mask` の計算 (`table/row.rs` の `mask`) が、rest の handle できるラベルを数えて、エフェクトごとの (n, m) を返す。`ls` に同じエフェクトがあるときの `UnifyError::MaskConflict` はなくす
- `BodyTypes::masks` (と推論の途中の `Typing::masks`) の値を、エフェクトごとの項目の並びにする。並びはエフェクトが rest に初めて現れた順で、Core IR への変換が並べ替える
- E2008 の報告 (`check/report.rs` の `MaskConflict` の枝)、その secondary だけに使う `signature_row_lists` (`check/body.rs`)、`codes::MASK_CONFLICT` を消す

### Core IR (`eml_core_ir`)

- `Rhs::Call` の `mask` の型を、項目の並びにする。項目は Core IR のエフェクトの番号と n と m を持ち、エフェクトの番号の昇順に並ぶ。項目の型は `eml_lir` に置き、`Prim::EvvMask` と共有する (私の判断)
- translate (`translate/expr.rs` の `mask`) は、型検査の項目をエフェクトの番号に写して昇順に並べる。`apply` を `mask` の変わる境目で分ける規則は、項目の並びの等しさで比べるだけで、今と変わらない
- EP の `mask` の包み (`evidence/handlers.rs` の `masked_call`)、`mask$` の補助の関数 (`evidence/helpers.rs`)、yield の地点の印 (`evidence/split.rs`) は、項目の並びをそのまま `prim evv_mask` の即値と `mask$` の名前に渡す。`Helpers::masks` の鍵も項目の並びにする。包みの形と遅い道は変えない
- verifier の `check_mask` は、エフェクトの番号が表にあること、番号が真に昇順であること (同じエフェクトの項目が2つないこと)、どの項目も m が 1 以上であることを確かめる。`mask` を付けられない命令の規則は今のままである

### LIR (`eml_lir`)

- `Prim::EvvMask` の即値を項目の並びにする。verifier (`check.rs` の `check_prim`) は Core IR と同じ3つを確かめる
- `Prim::row` の行 (`tobj` の借用の引数1つ、結果 `tobj`) は変えない

### テキストの形

項目は、エフェクトの名前 (今と同じく、入口のモジュールのエフェクトは修飾せず、ほかのモジュールのエフェクトは修飾する。表にない番号は `#N`) の後に、n が 0 でなければ `@n` を、m が 1 でなければ `*m` を続けて書く (私の判断)。

| 項目 | 書き方 |
|---|---|
| `State`、n = 0、m = 1 | `State` |
| `State`、n = 0、m = 2 | `State*2` |
| `Yield`、n = 1、m = 1 | `Yield@1` |
| `Yield`、n = 1、m = 2 | `Yield@1*2` |

- 使う場所は、Core IR の `mask [...]`、補助の関数の名前 `mask$[...]`、基本操作 `prim evv_mask[...]` の3つで、どれも同じ書き方である (`mask [State, Yield@1]`、`mask$[Yield@1]`、`prim evv_mask[Yield@1](w.3)`)
- `parse` は `@0`、`*0`、`*1`、0 で始まる数を誤りにする。読み直して表示すると表示が変わる形だからである (今の「空の `mask` は誤り」と同じ理由)。同じエフェクトの項目が2つあることと昇順でないことは、今と同じく `parse` でなく verifier が誤りにする
- 同じエフェクトを2回飛ばす今の書き方 (`mask [State, State]`) は、`State*2` になる
- IR の字句解析 (`eml_lir::syntax` の `lex`) は `Yield@1*2`、`#3@1`、REPL のセッションの中段の名前の `Log#3@1` を1つの語として読む。`parse` は語の末尾の `*m` と `@n` を外してから、エフェクトの名前か番号として引く。エフェクトの名前が `@` と `*` を含まないので、この読み方は一意に決まる。このことを testing.md のテキストの形の項に書く

### VM (`eml_vm`)

- `CodeBase` の `effect_lists` を、項目の並び (エフェクトの番号、n、m) を置く表にする。セッションでエフェクトを番号で引き直した後に昇順に並べ直す処理 (`compile/emit.rs`) は、項目 (エフェクト、n、m) を1つの単位として動かす
- 逆アセンブルの表示 (`disasm.rs`) に n と m を出す

### 実行時 (`eml_rt`)

- `Ctx::evv_mask` は、項目の並びを受ける。項目の型は、エフェクトの番号 (`u16`) と n と m を持つ `Copy` の構造体にする (私の判断。数の幅は実装で決める)
- 意味: `evv_seq(w)` の i 番目の組の tag が `L` に当たり、それより前に tag が `L` の組が s 個あるとき、`L` の項目の n と m について n ≤ s < n + m なら、その組を除く。項目のないエフェクトの組は残す
- `mask_spec` と、それを支える spec の関数 (`kept_at`、`wkept`、`wdropped` など) と補題を、この意味に書き直す。`mask_spec` はどんな項目の並びにも定め、同じエフェクトの項目が2つあれば先の項目を使い、m が 0 の項目は何も除かない。n と m は spec では自然数として扱い、実行のコードは n + m を桁あふれしない幅で比べる。昇順と重なりのなさと m が 1 以上であることは verifier が確かめ、`evv_mask` の前提には入れない (私の判断)。VM はセッションでエフェクトを番号で引き直すので、`evv_mask` に届く並びは verifier を通った並びそのものでないためである。速いモードの前提は「どの項目についても、tag の合う組が n + m 個以上ある」である。検査モードは、足りなければ今と同じく確保の前に `NoHandler` を返す
- 写す節点は、今と同じく除いた最後の節点までで、その後ろは元のリストを共有する。費用の上限の議論 ([オブジェクトモデル](../../spec/object-model.md) の「実行文脈 `Ctx`」の O(i·m + n·i)) は、項目の数で同じ形に書き直す。そこの n と m は項目の n と m と別の量なので、文字を替える。検査モードの前もってのたどりは、項目ごとに n + m 個の合う組を数える
- `cargo verus verify -p eml_rt` と `verusfmt --verus-only --check` を通す。`heap/tests/trusted.rs` の `every_primitive_once` の `evv_mask` の呼び出しを新しい引数の形にし、Miri の `trusted::` を通す

## テスト

### `run`

- 型を変換する handler。上の「目的と範囲」の `render` を、外側に `Yield String` の handler を置いて動かす。完了の条件である。次の2つが正しい handler に届くことを、出力で確かめる
  - `gen` が自分で起こす `Yield Int`。`render` の handler に届く
  - `e` を通る `Yield` の操作。`gen` をトップレベルの総称な関数 `gen : (Unit -> <e> Unit) -> Unit -> <Yield Int | e> Unit` にし、`gen` の中で row が `<e>` のコールバックを呼ぶ (`mask` は `Yield` を n = 0、m = 1)。`main` から `render (gen cb)` を、外側に `Yield String` と `Yield Bool` の handler を置いて呼び、`cb` の `Yield Bool` の `yield` が、`render` の handler と `Yield String` の handler を飛ばして `Yield Bool` の handler に届くことを確かめる。`gen` をラムダにしたり、`cb` に閉じた row の型を与えたりすると、`cb` の `Yield Bool` が `gen` の `Yield Int` と対になって型の誤りになるので、この形にする
- `k` の呼び出しに n と m がどちらも正の `mask` が付く形。たとえば、`L` の handler の節が、同じ `L` の handler を内側に設けてから、その中で `k` を呼ぶ。外側の row に `L` があるとき、再開した区間の `L` の操作が正しい handler に届くことを、出力で確かめる
- パラメータ付き handler と `multi` の操作でも、n と m がどちらも正の `mask` が付く形を1つずつ
- n と m がどちらも正の `mask` が付くのは、末尾が rigid な row 変数のシグネチャを持つトップレベルの関数の中だけである (たとえば `f : (Unit -> <e> Unit) -> <L | e> Unit` の中に handle と節の内側の handler を置く)。`main` では外側の row が閉じているので `mask` が付かず、注釈のないラムダでは `k` の row の末尾が推論用の変数なので `mask` が付かない。どのテストも、この形で書き、Core IR のダンプか単体テストで (n, m) の `mask` が付いていることも確かめる
- 今の `check-fail` の2つのテストを `run` に移す (下の「テストの変更」)

テストは `tests/ui/run/effects/` に置く。

### 単体テスト

- `eml_types`: `table/tests.rs` の `include_row` が、`ls` と rest に同じエフェクトがあるときに (n, m) を返すこと。`tests/masks.rs` に、上の「規則」の例の表の行を確かめるテスト
- `eml_core_ir`: `@n` と `*m` のテキストの読み書き (読み直して同じ表示になること、`@0`、`*0`、`*1` が誤りになること)、verifier の3つの規則、translate が付ける `mask`、EP が作る `mask$[Yield@1]` と `prim evv_mask[Yield@1]`
- `eml_lir`: `evv_mask` の即値のテキストの読み書きと verifier
- `eml_vm`: 手で書いた LIR で、`evv_mask[L@1]` が2つ目の evidence だけを除くこと
- `eml_rt` (`heap/tests/ctx.rs`): n が正の項目で、先頭の n 個を残して続く m 個を除き、残りのリストを共有すること。合う evidence が n + m 個に足りないときの検査モードの `NoHandler`。どちらも `both_modes!` で流す

## テストの変更

- 合否の変更 (種類1): `tests/ui/check-fail/types/mask_conflict.em` と `mask_conflict_handles.em` を `tests/ui/run/effects/` に移し、名前を `mask_behind.em` と `mask_behind_handles.em` に変える。どちらも、今は呼ばずにいる `outer` を `main` から2つの `Log` の handler の下で呼び、それぞれの `log` が届く handler を出力に表すように書き足す。E2008 がなくなったためである
- 合否の変更 (種類1): `crates/eml_types/tests/masks.rs` の `a_callee_that_performs_the_masked_effect_itself_is_rejected` と、`crates/eml_types/src/table/tests.rs` の `a_mask_cannot_skip_an_effect_the_callee_names` を、誤りでなく (n, m) の `mask` を確かめるテストに書き換える。名前も意味に合わせて変える。E2008 がなくなったためである
- 期待値の変更 (種類2): 同じエフェクトを2回以上飛ばす `mask` (`[State, State]`、`mask$[State, State]`、`EvvMask(vec![0, 0])` など) を含む期待値と、手で書いた IR のテキストを、`*m` の形に書き換える。`crates/eml_types/tests/masks.rs` (`a_label_twice_is_masked_twice` など)、`crates/eml_core_ir/tests/verify.rs` と `text.rs`、`crates/eml_core_ir/src/evidence/tests.rs`、`crates/eml_lir/tests/text.rs` と `verify.rs` が対象である。表記を変えたためである
- 期待値の変更 (種類2): `mask` が昇順でないことを確かめる verifier のテストの文言が、同じエフェクトの項目の重なりを含めた新しい規則の文言に変わる場合は、その期待値を書き換える
- 機械的な追従 (種類3): `mask` の型を項目の並びにしたことによる、期待値を変えないテストの書き換え (`include_row(..) == Ok(Vec::new())` の型、`eml_rt` のテストの `evv_mask(&[1])` の引数の形、`rt_bench` の `mask` など)

## 計測

- `bench/` のプログラムには `mask` がないので、`crates/eml_vm/tests/bench.rs` の `RunStats` は変わらないはずである。変われば原因を調べる
- `eml_rt` の `evv_mask` を書き直すので、段の終わりに `bench/rt.sh` の `mask` を測り、[計測の基準](../../implementation/benchmarks.md) に記録する

## 段の終わりの文書

- `docs/spec/types.md`: 「推論」の `mask` の決め方 (今の 137 行目と 138 行目。E2008 の項を消し、(n, m) の決め方と例に書き換える)。推論用の row 変数が末尾にある場合の注意
- `docs/spec/effects.md`: 「handler の意味」の `mask` の意味 (多重集合から項目へ) と、「健全性」の不変条件 (具体化した row で、先頭の部分の一致として述べる) と「呼び出し」の項
- `docs/spec/core-ir.md`: 呼び出しの `mask` の形、verifier、補助の関数の表の `mask$[…]` の名前、「`mask` の包み」、「`perform` と節の3つの形」のその場で動く節の議論
- `docs/spec/lir.md`: `evv_mask` の意味、即値、verifier の文言
- `docs/spec/object-model.md`: `mask_spec`、`evv_mask` の表の行、`&[u16]` の項、費用の議論
- `docs/spec/vm.md`: `effect_lists`、エフェクトの番号の引き直し
- `docs/spec/diagnostics.md`: E2008 の行を消し、E2007 と同じく欠番であることを書く
- `docs/implementation/diagnostics.md`: E2008 の項を消す
- `docs/implementation/architecture.md`: `BodyTypes::masks` の値
- `docs/implementation/testing.md`: `mask` と `evv_mask` のテキストの形、`heap/tests/ctx.rs` の筋書き
- `docs/future/verification.md`: `mask` の計算を述べる所から E2008 を除く
- `docs/implementation/status.md`: 今の言語の範囲
- `docs/future/roadmap.md`: S13b の節から S13b-1 の「決めたこと」を消し、表と分け方の表で S13b-1 を済みにする
- `CLAUDE.md`: `mask` と E2008 に触れる所

## 私の判断で決めたこと

- `mask` の項目の型を `eml_lir` に置き、Core IR の `mask` と `Prim::EvvMask` で共有する
- テキストの形を `L`、`L@n`、`L*m`、`L@n*m` にする。同じエフェクトを並べて繰り返す今の書き方はやめる
- `eml_rt` の `evv_mask` は、エフェクトの番号と n と m を持つ `Copy` の構造体の並びを受ける。`mask_spec` はどんな並びにも定め、並びの形を前提にしない
- 新しい `run` のテストの例を、n と m がどちらも正の `mask` が付く形 (末尾が rigid な row 変数のシグネチャを持つトップレベルの関数の中) に限る
- 移した2つの UI テストの名前を `mask_behind.em` と `mask_behind_handles.em` にし、`log` が届く handler を出力で示すように書き足す
- 型検査の `mask` の項目の並びは rest に現れた順のままにし、昇順に並べるのは Core IR への変換に任せる (今と同じ分担)
