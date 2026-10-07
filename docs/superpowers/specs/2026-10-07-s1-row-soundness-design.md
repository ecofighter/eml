# S1 row の健全性 (設計)

位置づけ: 作業の設計。再設計のサブプロジェクト S1 の spec である。全体の方針は [ロードマップ](../../future/roadmap.md) の「S1 row の健全性」にある。決まったことは S1 の終わりに `docs/` の該当文書へ移し、この文書を削除する。

## 目的

型検査が約束したエフェクトの行き先と、実行時に操作を捕まえる handler を一致させる。あわせて、handle できない `IO` が row の中で重なって起きる誤った E2002 をなくす。

## 背景

次のプログラムは `eml check` を通り、`eml run` で `internal error: a string operation on a value that is not a string in cb` を出して止まる。

```haskell
effect State s where
  get : Unit -> s
  put : s -> Unit

run : (Unit -> <e> a) -> <State Int | e> a
run cb =
  let n = get ()
  cb ()

cb : Unit -> <State String> String
cb () = get () ++ "!"

main : Unit -> <IO> Unit
main () =
  let r = handle (handle run cb with
                    | get () k -> resume k 42
                    | put _ k -> resume k ()) with
            | get () k -> resume k "str"
            | put _ k -> resume k ()
  println r
```

- `run` の本体で `cb ()` を呼ぶとき、型検査は `cb` の row `<e>` を今の row `<State Int | e>` に含める (`eml_types/src/table/row.rs` の `include_row` の、末尾が rigid な変数の分岐)。型の上では、`cb` のエフェクトは `State Int` を飛ばして、`e` に当たる外側の handler に届く
- 実行時の `perform` は、いちばん内側の `State` の handler を探す。そのため `cb` の `get` は `State Int` の handler に捕まり、`Int` を `String` として使って止まる

また、`<IO | e>` に `e := <IO>` が入ると row が `<IO, IO>` になる。これを `<IO>` の `main` から呼ぶと E2002 になり、help は、すでに書いてある `IO` を足すよう表示する。

## 決めたこと

### `mask` の意味

- `mask` は、呼び出しの間に起きた操作が handler を探すとき、その呼び出しより外側にある同じエフェクトの handler を、`mask` に含まれる数だけ飛ばす。呼び出しの中で設けた handler は飛ばさない (Koka の `mask` と同じ意味)
- `mask` はエフェクトの多重集合である。同じエフェクトを2回飛ばすなら、そのエフェクトを2つ持つ

### 型検査

- 呼び出し先の矢印の row `<ls | e>` (`e` は rigid な row 変数) を今の row に含めるとき、`ls` を今の row のラベルと順に対にした残りのうち、`e` の手前に余ったラベルを rest と呼ぶ。rest から handle できないラベルを除いたものを、その矢印の `mask` にする
- rest のラベルのエフェクトが `ls` にも現れたら、E2008 にする。呼び出し先が自分で起こす `L` は1つ目の `L` の handler に届き、`e` を通る `L` は rest の `L` をすべて飛ばす必要があるが、`mask` は呼び出しの中の `L` の操作をすべて同じだけ飛ばすので、両方を同時には満たせないためである
- `mask` は、`BodyTypes` の side table に記録する。キーは呼び出しの式と矢印の番号の組か、`resume` の式である。並びが空の `mask` は記録しない
- row を展開するとき (`resolve_row`)、handle できないラベルの2つ目以降を落とす。展開した row を使う単一化、包含、表示のすべてに、この規則が効く
- S1 で handle できないラベルは、組み込みの `IO` (`lang.io`) だけである。S2 で、extern のエフェクトに広げる
- `IO` は型引数を持たないので、まとめても型引数の食い違いは起きない。`IO` の操作はどれも実行時の組み込みの handler が処理するので、どちらの `IO` に届いても意味は同じである

### Core IR

- 呼び出し (`Rhs::Call` の `Call::Direct`、`Call::Apply`、`Call::Resume`) と末尾呼び出し (`CExpr::TailCall`) に、`mask` を持たせる。`mask` はエフェクトの番号の昇順に並べた多重集合で、空なら `mask` なしである。`Call::Handle` と `Call::Perform` は `mask` を持たない。`handle` の本体の中の呼び出しは、持ち上げた本体の関数の中の呼び出しとして `mask` を持つ
- translate は、HIR の呼び出しから作る Core IR の呼び出しに、型検査が記録した矢印ごとの `mask` を付ける
  - 引数がそろう既知の関数の呼び出し (`Call::Direct`) は、最後の矢印の `mask` を使う。前の矢印は部分適用で、エフェクトを起こさないためである
  - 余った引数を `Call::Apply` でまとめて渡すときは、矢印ごとの `mask` が変わる境目で `apply` を分ける。`mask` は1つの Core IR の呼び出し全体に効くので、違う `mask` の矢印を1つの `apply` にまとめると、片方の矢印に余計な `mask` が効くためである
  - `mask` が効くのは呼び出しそのものだけで、引数の評価には効かない。ANF では引数は呼び出しより前に評価済みなので、これは形から決まる
- `mask` は末尾かどうかと独立である。`mask` 付きの呼び出しも末尾の位置では末尾呼び出しにし、[Core IR とインタプリタ](../../spec/core-ir.md) の「末尾の呼び出しは末尾呼び出しにする」という規則に例外を作らない。simplify の T 規則は、`mask` を保ったまま末尾呼び出しにする
- `mask` は値を持たないので、Perceus、生存解析、`saved` の規則は変わらない
- verifier は、`mask` のエフェクトの番号がエフェクトの表にあること、昇順に並んでいること、`IO` を含まないことを確かめる
- テキストの形は、呼び出しの前に `mask[エフェクト名, …]` を書く。エフェクト名は `pretty` が表示する修飾した名前で、番号の順に、数だけ繰り返す

  ```
  let t = mask[Main.State] apply cb(()) [x]
  let t = mask[Main.State, Main.State] call helper(cb)
  tailcall mask[Main.State] apply cb(())
  let t = mask[Main.Log] resume k(v, ())
  ```

  `mask[` は、関数 `mask` の直接の呼び出し (`mask(`) と区別できる

### インタプリタとランタイム

- ランタイムのフレームに `Frame::Mask { effects, next }` を足す。`effects` は `mask` の多重集合で、`next` のほかに値を持たない
- `mask` 付きの呼び出しは、`Mask` フレームを1つ積んでから呼ぶ。末尾でない呼び出しでは戻りのフレームの上に積み、末尾呼び出しでは戻りのフレームの代わりに積む。`resume` では、`resume` が今の継続を読む前に積む
- 値が `Mask` フレームに届いたら、フレームを外して値をそのまま次へ返す
- `find_handler` は、探すエフェクトについて飛ばす数を数える。`Mask` フレームではその数に `effects` の中の数を足し、同じエフェクトの handler フレームでは、数が正なら1減らして飛ばす
- `Mask` フレームは、ほかのフレームと同じく継続の区間に入り、`multi` の再開では写され、中断では解放される。`copy_segment`、子のたどり方、`debug_heap` のリークの数え方は、引数のない `Apply` フレームと同じに扱う
- 末尾の `mask` 付き呼び出しで、継続の先頭がすでに `Mask` フレームなら、2つを1つのフレームに併合しても意味は変わらない。ただし S1 では併合しない。`Mask` フレームの数は handler フレームの数以下に収まり (下の「健全性」)、併合の効果は定数倍にとどまるためである。併合は、許される最適化として [コンパイラの構成](../../implementation/architecture.md) の「継続のフレーム」に書く
- S3b で `perform` が handler だけをたどる連鎖に作り直すときは、handler フレームと `Mask` フレームを連鎖の節にする。evidence passing では、`mask` は evidence の項目を削って呼び、戻す包み (Koka の `mask-at`) に変換する。どちらも、S1 の呼び出しごとの `mask` をそのまま使う

### 健全性

[エフェクトと handler](../../spec/effects.md) に節を足し、次の不変条件を示す。

> 実行中のある点で、継続を外側に向かってたどり、`Mask` フレームを数えて handler を飛ばしたとき、見える handler の並びは、その点の静的な row の handle できるラベルの並びと、先頭から一致する。

- 呼び出し: 呼び出し先の row `<ls | e>` の `ls` は、今の row の先頭からの対になるラベルと一致する。rest のラベルは `ls` に現れない (E2008) ので、`mask` は `ls` の操作に効かない。`e` の操作は rest の handler をすべて飛ばし、今の row の `e` に当たる handler に届く
- `handle`: 本体の row は、扱うエフェクトのラベルを今の row の先頭に足したものである。handler フレームはその先頭の handler になる
- 節と `resume`: 節は handle の外側の row で動き、継続の区間を切り離した後の継続も handle の外側である。`resume` の `mask` は、継続の row を今の row に含めるときの rest で、呼び出しと同じ議論になる
- `IO`: 実行時の組み込みの handler だけが処理するので、row の中の `IO` の数は handler の選び方に関わらない

この不変条件から、操作はその静的な row のラベルが約束した handler に捕まる。また、`L` を飛ばす `mask` の各項目は、同じ継続の上の別々の `L` の handler に対応するので、`Mask` フレームの数は handler フレームの数以下に収まる。

### E2008

- 番号は E2008、名前は `MASK_CONFLICT` とする
- primary は呼び出しの式 (`resume` なら `resume` の式) である。メッセージは、呼び出し先が自分で起こす `L` と、row 変数を通って外側の handler に届くべき `L` を区別できない、という趣旨にする
- 今の row で余った `L` の由来がシグネチャの row なら、secondary でそのシグネチャの矢印を指す
- 細かい文言は [診断の出し方](../../implementation/diagnostics.md) に書く

## 対象外

- `Mask` フレームの併合 (上の「インタプリタとランタイム」)
- `perform` が handler だけをたどる連鎖 (S3b)
- 同じエフェクトのインスタンスを区別する名前付きの handler (可変参照を入れるとき)
- handle できないラベルを extern のエフェクトに広げること (S2)

## テスト

### 足すテスト

UI テスト (`tests/ui/`):

| テスト | 確かめること |
|---|---|
| `run/effects/mask_callback.em` | 背景のプログラム。`cb` の `get` が外側の `State String` に届き、`str!` を出す |
| `run/effects/mask_inside_handler.em` | ラッパーが自分の中で `State` を handle しつつ利用者のコールバックを呼ぶ。コールバックの `State` が内側の handler に取られず、外側に届く |
| `run/effects/mask_multi.em` | `mask` を含む区間を持つ `multi` の継続を2回再開する。どちらの再開でも `mask` が効く |
| `run/effects/mask_never.em` | `never` の操作が `Mask` フレームを越えて中断する。`debug_heap` でリークがない |
| `run/effects/io_twice.em` | `<IO \| e>` に `<IO>` が入る形 (`try` と `with_env` で包んだ deploy の形) が通る |
| `check-fail/types/mask_conflict.em` | E2008 |

段階ごとのテスト:

| crate | 確かめること |
|---|---|
| `eml_types` | 記録される `mask` (矢印ごと、`resume`、`IO` を除くこと、並びが空なら記録しないこと、同じエフェクトを2回飛ばすこと)、`IO` の重なりをまとめた表示、E2008 |
| `eml_core_ir` | translate のダンプに `mask[…]` が出ること、矢印ごとの `mask` が変わる境目で `apply` が分かれること、末尾の `mask` 付き呼び出し、テキストの読み戻し、verifier の誤り (知らないエフェクト、昇順でない並び、`IO`) |
| `eml_interp` | Core IR のテキストで、`Mask` フレームが handler を飛ばすこと、呼び出しの中で設けた handler は飛ばさないこと、`mask` 付きの末尾呼び出し、`mask` 付きの `resume` |
| `eml_runtime` | `Mask` フレームを含む区間の複写と解放 |

### テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。

- 成否の変更: なし
- 期待値の変更: `eml_types` の単体テスト `table::tests::labels_of_one_effect_pair_up_in_order` を書き換える。このテストは、型引数を持つ仮のエフェクトとして `IO` を借りている。`IO` の重なりをまとめる規則と合わないので、handle できるエフェクトに替える。確かめる内容 (同じエフェクトのラベルを、それぞれの row の中の順で対にすること) は変えない
- 機械的な追随: `CExpr::TailCall` と `Rhs::Call` に `mask` の欄を足すことに伴う、Core IR の構築とパターンの書き換え。`mask` のない呼び出しの表示は今と同じなので、スナップショットは変わらない

使い捨ての worktree で `include_row` に記録を足して全テストを流した範囲では、`mask` が付く既存のテストはなかった (余るラベルは `IO` だけだった)。`IO` の重なりをまとめて落ちる既存のテストは、上の1件だけだった。

## 更新する文書

S1 の終わりに、次の文書を直す。

| 文書 | 直すこと |
|---|---|
| `docs/spec/types.md` | 「推論」に、row の包含と `mask`、handle できないラベルの重なりをまとめる規則を書く |
| `docs/spec/effects.md` | 「handler の意味」に `mask` の意味を足し、「健全性」の節を足す |
| `docs/spec/core-ir.md` | 呼び出しと末尾呼び出しの `mask`、インタプリタでの `mask` の意味を書く |
| `docs/spec/diagnostics.md` | E2008 を「割り当て済みの番号」に足す |
| `docs/implementation/architecture.md` | 「継続のフレーム」に `Mask` フレーム、`find_handler` の数え方、許される最適化としての併合を書く。`eml_types` の内部に `mask` の side table を書く |
| `docs/implementation/testing.md` | 「Core IR のテキストの形」に `mask[…]` を書く |
| `docs/implementation/diagnostics.md` | E2008 の表示を書く |
| `docs/implementation/status.md` | 既知の制限から、`<IO, IO>` の E2002 の項目を消す |
| `docs/future/roadmap.md` | 「S1 row の健全性」の節を消し、段の列から S1 を消す |

## 完了の条件

- 背景のプログラムが `str!` を出す run テストになる
- `<IO | e>` に `<IO>` が入る形のプログラムが通る
- 上の足すテストがすべてあり、`cargo test` がすべて通る
- 上の文書を更新し、引用の検査が通る
