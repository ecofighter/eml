# 縦の貫通 段階5b: 持ち越し規則と中断時の後始末の設計

位置づけ: 作業用の設計文書。段階5b を終えたら、残す価値のある内容を `docs/spec/`、`docs/implementation/architecture.md`、`docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

[status.md](../../implementation/status.md) の「名前解決以降の実装段階」の段階5b を通す。

### 5b に含めるもの

- 持ち越し規則 ([エフェクトと handler](../../spec/effects.md) の「継続の多重度と持ち越し規則」) の検査。`multi` の操作を起こしうる呼び出しをまたいで `Lin` の値を持つことを、コンパイル時に拒否する
- Kind が Kind 変数の値のための、条件つきの持ち越しの制約と、そのスキームへの残し方
- 持ち越し規則の違反の診断 E3006
- 扱うエフェクトに `multi` の操作がある handler で `return` の節の捕獲を `Unr` にする今の規則を、持ち越し規則にまとめること
- 外側の `multi` の handler が内側の handler フレームを写したときに、内側の `return` の節が2回動く問題 (`status.md` の段階5b の注意点)
- 中断時の後始末の確認。`drop k` と `never` の操作で区間を解放するときに、捕まっていた `File` が解放されることを、実行テストで確かめる
- `tests/ui/run/effects/multi_over_once.em` を `check-fail/linearity/` に移すこと

### 後に回すもの

- パラメータ付き handler の状態の持ち越し (段階6)。状態は handler フレームの欄にあるので、段階6で handler フレームを「本体の実行をまたいで持つ値」に加える
- 開いた row の呼び出しの精度 (下の「精度の限界」)。`let` で束縛したラムダの row を閉じて使う位置で開くか、row の包摂を入れるときに見直す

### ユーザーと合意済みの決定

- Kind が Kind 変数の値には、条件つきの制約「値が `Lin` なら σ ≤ Once」を入れる (案A)。多重度の側を保守的に縛る案と、線形性の側を保守的に縛る案は、多相な高階関数の使い道を `multi` か `Lin` のどちらかで削るので採らない
- 「またいで持っている値」には、名前のある変数に加え、評価済みで消費前の部分式の値 (途中の値) を含める。変数だけでは `consume f (choose ())` を通してしまい、健全でないためである
- 規則を当てはめる形は、関数とクロージャの呼び出し、操作の直接の呼び出し、`resume`、`handle` とする (下の「規則を当てはめる形」)
- 操作を直接呼ぶときは、エフェクトの単位ではなく、その操作の多重度で判定する
- 開いた row の呼び出しが今の row と同じとみなされる精度の限界は、段階5b では直さず、既知の制限として記録する
- 違反は新しい番号 E3006 にし、D1〜D3 の形で報告する。同じ値は最初の呼び出しの1件だけを報告する
- `return` の節の捕獲の専用の規則 (E3001、`CapturedByReturnClause`) は消し、持ち越し規則にまとめる。既存のテストの期待値が変わる (種類1)

## 1. 健全性の根拠

`multi` の継続を2回目に再開すると、区間のフレームが写され、フレームが持つ値が共有になる ([Core IR とインタプリタ](../../spec/core-ir.md))。写されて共有になる値は、次のフレームが持つものに限られる (`eml_runtime/src/heap.rs` の `Frame`)。

| フレーム | 持つ値 | 対応するソースの形 |
|---|---|---|
| `Return` | 呼び出しの後で使う値 (`saved`) | 呼び出し、操作、`handle`、`resume` の後の続き |
| `Apply` | 余った引数 | 矢印の数より多い引数を渡した呼び出し |
| `Handler` | 節のクロージャと `return` の節のクロージャ | 区間の中にある内側の `handle` |

あるフレームが `multi` の操作の区間に入るのは、そのフレームを積んだ呼び出しの最中に、その呼び出しの外へ抜ける `multi` の操作が起きたときに限られる。外へ抜ける操作のエフェクトは、その呼び出しの row に現れる。そのため、呼び出しごとに、row と「その時点で持っている値」を照らし合わせれば、上の3つのフレームをすべて覆える。

値の中に入れ子になった `Lin` の値は、値の Kind が覆う。クロージャは捕まえた値の Kind 以上になり、`data` とタプルはフィールドの Kind の join になり、`once` の操作の `k` は `Lin` である ([型と Kind](../../spec/types.md))。

### 操作の直接の呼び出し

エフェクト `E` が `once` の操作 `ask` と `multi` の操作 `m` を持つとき、`ask ()` をまたいで持っている値は、`m` の区間に入らない。`ask ()` のフレームは `ask` の区間の先頭にあり、`resume` されると値をすぐに受け取って消えるためである。`ask` の節の中で `m` を起こしても、節は `ask` の `k` を値として持っているので、`k` (`Lin`) を持ったまま `multi` をまたぐことになり、節の側で拒否される。そのため、操作を直接呼ぶときは、その操作の多重度だけを見ればよい。`never` の操作をまたいだ `Lin` の値は、中断のときに区間と一緒に解放されるので、拒否しなくてよい。

## 2. spec の変更

### `effects.md` の「継続の多重度と持ち越し規則」

- 「呼び出しをまたいで `Lin` 変数が生きている場合」を「呼び出しをまたいで `Lin` の値を持っている場合」にする。「持っている値」は、呼び出しの後の評価で使う局所変数と、評価済みで呼び出しの後に消費される部分式の値 (途中の値) である。評価の順は、Core IR の変換と同じく、呼ばれる式、引数の順に左からとし、`x |> f` の `x` だけを先に評価する ([宣言](../../spec/declarations.md))。
- 規則を当てはめる形と、見る row を表にする (下の「規則を当てはめる形」と同じ表)。
- 操作を直接呼ぶときは、その操作の多重度だけを見る、と書く。理由は上の「操作の直接の呼び出し」を短くしたものにする。
- 値の型の Kind が Kind 変数を含むときは、条件つきの制約 (`types.md`) にする、と書く。
- 「この検査は各呼び出し箇所で局所的に行えば、全体として健全になる」の後に、上の「健全性の根拠」を短く書く。
- 「handler の意味」の、`multi` の操作を扱う handler の `return` の節の捕獲の文を、持ち越し規則を参照する形に書き換える。`return` の節が捕まえた値は、本体の row 全体をまたいで持つ値として持ち越し規則にかかる。

### `linearity.md`

- 「基本の規則」の、扱うエフェクトに `multi` の操作があれば `return` の節の捕獲に `Unr` の制約を加える文を消し、「`return` の節が捕まえた値は、持ち越し規則にかかる ([エフェクトと handler](effects.md))」に置き換える。
- 「線形性の検査パス」に、持ち越し規則を使用回数のパスの直後の別のパスで行うこと (下の「4. 持ち越しのパス」) と、E3006 を足す。

### `types.md` の「推論」

持ち越しの制約の形、解き方、スキームへの残し方を書く (下の「3. 持ち越しの制約」)。

### `diagnostics.md`

- E3006 `LINEAR_VALUE_KEPT_ACROSS_MULTI` の行を足す。「持ち越し規則の番号は段階5b で割り当てる」を消す。
- E3001 の説明から「`multi` の操作を持つ handler の `return` の節が捕まえた場合を含む」を消す。
- 番号の割り当ての表の「`multi` の呼び出しをまたぐ線形な変数 (段階5b)」を消す。
- 「線形性の診断」の表の「`multi` の呼び出しをまたぐ」の行を、下の「5. 診断」の D1〜D3 の指す場所に書き換える。

### `core-ir.md`

「中断時の後始末の確認は段階5b で行う」を、確かめた内容 (`drop k`、`never` の操作、`return` の節のクロージャを持つ handler フレームの解放で、捕まっていた `File` が解放される) に書き換える。

### `status.md` と `test-changes.md`

- 段階5b を完了にし、段階5b の注意点 (σ の上限、`multi_over_once`、内側の `return` の節、使用回数のパス、handler フレームの解放) を消す。
- 精度の限界を、既知の制限として例と一緒に記録する (下の「精度の限界」)。
- `test-changes.md` に、種類1の変更を記録する (下の「7. テスト」)。

## 3. 持ち越しの制約

### 形

```
carry(l, s, 由来)
  意味: l の解が Lin なら、s の解は Once 以下でなければならない
  l: Bound<Linearity>    持っている値の型の Kind の成分
  s: Bound<Multiplicity> 呼び出しの row の成分
```

- `l` は、値の型を `Table::kind_bounds` で分解した成分である。定数 `Lin` (`File` など)、rigid な型変数の `μ`、矢印の `m` などがある。
- `s` は、呼び出しの row の成分である。ラベルごとにそのエフェクトの多重度の定数、末尾が row 変数ならその `σ` である。末尾が閉じているか `Error` なら、末尾の成分はない。操作の直接の呼び出しでは、その操作の多重度の定数だけである。
- 成分の組ごとに1つの制約を出す。`l` が定数 `Unr` の組と、`s` が定数 `Never` か `Once` の組は、破れないので出さない。
- row に `multi` のラベルがある場合も、線形性の束の `kind(v) ≤ Unr` にはせず、`s` が定数 `Multi` の持ち越しの制約にする。こうすると、多相な `a` の値が具体的な `multi` の呼び出しをまたいだときも、スキームに持ち越しの制約として残る。呼んだ側で違反したときに、E3001 (`Passed`) ではなく E3006 で報告できる。

`Table` は持ち越しの制約を、線形性と多重度の束とは別の表に持つ。`Lattice` の制約は由来をスキームに残さないが、持ち越しの制約はスキームに残すときも由来を持つためである。

### 解き方

線形性の解は多重度に依存しないので、`Table::solve_kinds` で次の順に解く。

1. 線形性の束を解く (今のまま)
2. 多重度の束を解く (今のまま)
3. 持ち越しの制約ごとに、`l` の解が `Lin` で `s` の解が `Multi` なら違反とし、その由来を違反の由来に加える

持ち越しの制約は上限を足すだけで、どちらの束の最小解も動かさない。そのため、3 は解いた後の確認で済む。由来のない制約 (誤りのある本体で出したもの) は、今と同じく報告しない。

### スキームへの残し方

`Scheme::generalize` で、持ち越しの制約を、残す Kind 変数 (`Table::kind_vars` が返すもの) についての制約に置き換えて残す。

- `l` の側は、`l` から下向きにたどって届く、残す線形性の変数と定数 `Lin` の集合にする。`l` の解は下限の join なので、`l` が `Lin` になるのは、そのどれかが `Lin` のときに限られる。
- `s` の側は、`s` から下向きにたどって届く、残す多重度の変数と定数 `Multi` の集合にする。`s` が `Once` 以下であるのは、下限のすべてが `Once` 以下のときに限られる。
- 2つの集合の組ごとに、元の由来を持ったまま残す。両方が定数の組 (`Lin` と `Multi`) は、その本体の中の違反で、上の 3 が報告するので残さない。
- 下向きにたどる処理は、`Lattice::residual` の `reach` と同じものを使う。

`Scheme::instantiate` は、今の線形性と多重度の制約と同じく、残した Kind 変数を新しい変数に置き換えて複写する。複写した制約の由来は `CarriedThrough { name, inner }` にする (下の「5. 診断」)。`name` は参照した値の名前、`inner` はスキームに残した元の由来、由来の範囲は参照した位置である。今の `Passed` と同じく、参照した位置の由来は、本体の検査が具体化のときに設定したものから取る。

SCC の検査を終えるまでは Kind 変数を多相化しないので、同じ SCC の中の参照は、今と同じく Kind 変数を共有する。

### 表示

`TypedModule` のスキームの制約 `KindConstraint` を、次の enum にする。

```rust
pub enum KindConstraint {
    /// 線形性の制約 `lower <= upper` (今の構造体と同じ)。
    Linearity { lower: KindTerm, upper: KindTerm },
    /// 持ち越しの制約。`value` が Lin なら `row` は Once 以下。
    Carry { value: KindTerm, row: RowTerm },
}

pub enum RowTerm {
    Multi,
    /// row 変数。表示は `<e>`。
    Of(String),
}
```

表示は `a => <e> <= Once` とする。`value` が定数 `Lin` なら `<e> <= Once`、`row` が `Multi` なら `a => Multi <= Once` とする。今の線形性の制約の表示と同じく、名前のない Kind 変数を含む制約は表示しない。`status.md` で確かめ残していた「row 変数の σ の上限がスキームに残り、具体化で複写される」ことを、この表示と D3 のテストで確かめる。

## 4. 持ち越しのパス

### 規則を当てはめる形

| 形 | 見る row | またいで持っている値 |
|---|---|---|
| 関数とクロージャの呼び出し | 型検査がたどった矢印ごとの row | 呼び出しの後で使う値。後の矢印に渡す評価済みの引数を含む |
| 操作の直接の呼び出し (`ask ()`) | 操作の多重度。操作の引数の数より後の矢印は、その矢印の row | 同上 |
| 操作を値として呼ぶ (`let g = ask` の後の `g ()`) | 値の型の row (エフェクトの単位) | 同上 |
| `resume k v` | `k` の型の、handle の外側の row | 節の中で後で使う値 |
| `handle e with ...` | 外側の row `ρ` | handle の後で使う値 |
| 同上 | 本体の row `<E \| ρ>` | `return` の節が捕まえた値 |

規則を当てはめないものと、その理由は次のとおり。

- `handle` の本体が捕まえた値は、本体のクロージャに移るので、持ち越しにならない。本体の中の呼び出しは、本体の中で検査する。
- 操作の節が捕まえた値は、今の規則で `Unr` に縛られている。
- row が空の呼び出し (純粋な関数、コンストラクタ、`==`、引数のないトップレベルの値) は、制約を出さない。
- `jump` と `drop` はフレームを積まない。
- 部分適用は実際には呼び出しが起きないが、型検査が矢印をたどるので、最初の矢印の row が空でない関数を部分適用したときだけ厳しめに判定する。健全なので、そのままにする。

### 型検査が記録するもの

`BodyTyping` に、呼び出しの row を記録する表を足す。

```rust
pub(crate) enum CallRows {
    /// 呼び出し。たどった矢印ごとの row。呼ばれる式が操作の名前なら、その操作も持つ。
    Call { arrows: Vec<Row>, operation: Option<OperationId> },
    /// `resume k v`。`k` の型の、handle の外側の row。
    Resume(Row),
    /// handle。本体の row と外側の row。
    Handle { body: Row, outer: Row },
}

pub(crate) struct BodyTyping {
    // 今のフィールドに加えて
    pub calls: ArenaMap<ExprId, CallRows>,
}
```

- 呼び出しの矢印ごとの row は、`include_call_row` に渡す row である。矢印が `Arrow::Error` のときは記録しない。
- row は記録したまま持ち、持ち越しのパスが表から解く。
- 型の誤りを報告済みの本体でも記録する。持ち越しのパスは由来を記録しないので、報告はされない。

### 走査

新しいモジュール `eml_types::carry` に置く。`check/mod.rs` が、`usage::constrain` の直後に、同じ本体と同じ `reliable` の判定で呼ぶ。`reliable` の判定は `usage` から取り出して両方で使う。使用回数のパスは評価の順を区別しないので、同じ走査に混ぜず、評価の順に沿った生存の計算を別に持つ。

- 式を評価の順の逆にたどり、各位置で「後で使う局所変数」の集合を求める。分岐 (`if` と `match`) では、枝ごとに求めた集合を合わせる。
- 呼び出し、タプル、`resume` の部分 (呼ばれる式と引数、要素、`k` と値) を評価している間は、それより前に評価した部分の値を、消費前の途中の値として持つ。途中の値は、その部分式の `ExprId` で表し、型は `BodyTyping::exprs` から取る。
- 呼び出しの位置で、後で使う局所変数と途中の値のそれぞれについて、型の Kind の成分と row の成分の組で持ち越しの制約を出す。局所変数の型は `BodyTyping::locals` から取る。矢印ごとの呼び出しでは、後の矢印に渡す引数も途中の値に入れる。
- ラムダの本体、handle の本体、操作の節、`return` の節は、持ち上げる関数なので、それぞれの本体で、後で使う値が空の状態から始める。
- `handle` の位置では、`return` の節が捕まえた変数 (`Body::captures`) を本体の row と、handle の後で使う値を外側の row と組にする。
- 由来は、`reliable` が真のときだけ記録する。

### 使用回数のパスから消すもの

`usage.rs` の、扱うエフェクトに `multi` の操作があれば `return` の節の捕獲を `Unr` にする規則と、`KindReason::CapturedByReturnClause` を消す。

## 5. 診断

### 由来

`KindReason` に次の2つを足す。

```rust
/// 呼び出しをまたいで持っている値。由来の範囲は呼び出しの範囲。
CarriedAcross {
    value: CarriedValue,
    /// 報告するときに解く row。操作の直接の呼び出しでは、その操作。
    across: Across,
    /// 呼び出しの種類。primary のラベルの言い方を決める。
    call: CallKind,
},
/// スキームから複写した持ち越しの制約。由来の範囲は参照した位置。
CarriedThrough { name: String, inner: Option<Box<KindOrigin>> },

enum CarriedValue {
    /// 局所変数。名前と束縛の範囲。
    Local { name: String, binding: TextRange },
    /// 途中の値。部分式の範囲。
    Temporary(TextRange),
    /// `return` の節が捕まえた変数。名前、束縛の範囲、節の範囲。
    ReturnCapture { name: String, binding: TextRange, clause: TextRange },
}

enum Across { Row(Row), Operation(OperationId) }
enum CallKind { Call, Resume { k: String }, Handle }
```

由来は `PartialEq` で比べて重複を除くので、`Row` も比べられるようにする。

### E3006 `LINEAR_VALUE_KEPT_ACROSS_MULTI`

**D1 と D2** (`CarriedAcross`)。D1 は呼び出しの row に `multi` のラベルがある場合、D2 は同じ本体の中で row 変数が `multi` を含む row に単一化された場合である。報告の時点では row が解けているので、どちらも同じ形で報告する。

- メッセージ: 「`f` must be used exactly once, but it is kept alive across a call that may resume more than once」。途中の値では `` `f` `` の代わりに「a linear value」とする。
- primary: 呼び出しの範囲。row を解き、`multi` の操作を持つ最初のラベルのエフェクトから、宣言の順で最初の `multi` の操作を選ぶ。ラベルは「this call may perform `choose`, a `multi` operation」とし、`handle` では「this handle may perform ...」、`resume` では「resuming `k` may perform ...」とする。操作の直接の呼び出しでは、その操作を選ぶ。
- secondary: 値の側。局所変数なら束縛「`f` is bound here」、途中の値ならその部分式「this value is kept alive across the call」、`return` の節の捕獲なら節「the `return` clause captures `f`」とする。
- secondary: 選んだ操作の宣言 (`Operation::name_range`)「`choose` is declared `multi` here」。
- row を解いても `multi` のラベルが見つからないときは、操作の secondary を省き、primary のラベルを「the row of this call may include `multi` operations」とする。
- note: 「a continuation of a `multi` operation can be resumed more than once, and each resumption would use the value again」。
- help: 「finish using `f` before this call」。途中の値では「finish using the value before this call」。

**D3** (`CarriedThrough`)。呼んだ関数のスキームから複写した持ち越しの制約が、呼んだ側で破れた場合である。

- メッセージ: 「`keep` keeps a linear value alive across a call that may resume more than once」。
- primary: 参照した位置「`keep` is used here」。
- secondary: `inner` の範囲。`inner` が `CarriedAcross` なら「`x` is kept alive across this call」(途中の値では「a value is kept alive across this call」)、`CarriedThrough { name: g, .. }` なら「through this use of `g`」とする。たどるのは1段だけにする。`inner` がなければ secondary を出さない。
- note: D1 と同じ。

**同じ値の重複**: 同じ値 (局所変数と `return` の節の捕獲は束縛の範囲、途中の値はその範囲で見分ける) の `CarriedAcross` の違反は、呼び出しの範囲が最も前のもの1件だけを報告する。`check/mod.rs` で `solve_kinds` の結果を報告に変える前にまとめる。D3 はまとめない。

`report::linear_misuse` から `CapturedByReturnClause` の枝を消し、上の2つの枝を足す。

## 6. 精度の限界

`include_call_row` は、末尾が推論用の変数の row を、今の row とまるごと単一化する (`table/row.rs` の `include_row`)。そのため、`multi` のエフェクトを起こさない呼び出しでも、今の row に `multi` があれば、またいで持っている `Lin` の値が拒否される。

```haskell
search : Unit -> <Choice, IO> Unit
search () =
  let f = open "a.txt"
  let log = fn () -> println "x"
  log ()        -- log の row が <Choice, IO> と単一化されるので、f を持ったまま multi をまたぐと判定される
  close f
```

エフェクト多相な関数に純粋な関数を渡す `keep f (fn () -> ())` も、今の row に `multi` があれば同じく拒否される。判定は健全な側に外れるだけで、誤ったプログラムを通すことはない。原因は、`let` で束縛したラムダの row が最初の呼び出しで決まる既知の制限と同じである。`status.md` の「次の作業の注意点」に、この例と一緒に既知の制限として記録する。

## 7. テスト

### `eml_types` のテスト

`crates/eml_types/tests/linearity.rs` に、拒否するものと通すものの対を足す。

拒否するもの (E3006):

- `File` を持ったまま `choose ()` を呼ぶ (D1)
- 途中の値: `consume f (choose ())` と `(f, choose ())`
- row が閉じた `<Choice>` の関数の呼び出しをまたぐ
- `once` の節で、`k` を持ったまま `choose ()` を呼ぶ (`multi_over_once` と同じ形)
- 外側の row に `Choice` がある `resume k ()` を、節の引数の `File` を持ったまままたぐ
- 本体が `choose` を起こす内側の `handle` 式を、`File` を持ったまままたぐ
- `return` の節が `File` を捕まえる。外側の `multi` の場合と、扱うエフェクト自身の `multi` の場合
- D2: `let` で束縛したラムダの row が今の row と単一化される。精度の限界として拒否されることを記録するテストにする
- D3: `keep file choose_action`。自分で開いた `File` を持ったままコールバックを呼ぶ関数に `multi` の動作を渡す場合。2段の D3 (`keep` を呼ぶ `keep2`)。`keep_choose : a -> <Choice> a` に `File` を渡す場合
- 同じ値が `multi` の呼び出しを2回またいでも、報告は1件になる

通すもの:

- `File` を持ったまま、`once` の操作、`never` の操作、`IO` の呼び出しをまたぐ
- `multi` の操作も持つエフェクトの `once` の操作をまたぐ
- `keep 1 multi_action` と、純粋な row の中での `keep file pure_action`
- `multi` の呼び出しの前に `File` を閉じる、または引数として渡す
- `Int` の値と `multi` の `k` (`Unr`) を持ったまま `multi` をまたぐ

スキームの表示のテストで、`keep` に `a => <e> <= Once` が、自分で `File` を開いてコールバックをまたぐ関数に `<e> <= Once` が、`keep_choose` に `a => Multi <= Once` が残ることを確かめる。

`kind.rs` の単体テストで、持ち越しの制約の解き方 (`l` と `s` の解の組み合わせ) と残し方 (内部の変数を通した置き換え、定数の組の扱い) を確かめる。

### UI テスト

- `check-fail/linearity/`
  - `multi_over_once.em` を移す (種類1)。冒頭のコメントを、持ち越し規則で拒否されることの説明に書き直す
  - `file_across_multi.em` (D1 の代表) を足す
  - `file_through_polymorphic_function.em` (D3 の代表) を足す
  - `return_clause_across_multi.em` (外側の `multi` での `return` の節の捕獲) を足す
- `run/files/` (中断時の後始末。実行テストはつねに `debug_heap` が有効なので、解放漏れはリークとして失敗する)
  - `file_dropped_with_continuation.em`: `once` の操作をまたいで持っていた `File` を、`drop k` で区間と一緒に解放する
  - `file_released_on_abort.em`: `never` の操作をまたいで持っていた `File` を、中断で解放する
  - `file_in_return_clause_on_abort.em`: `return` の節が捕まえた `File` を、中断で handler フレームと一緒に解放する。操作の節は `Unr` の値しか捕まえられないので、handler フレームに `File` が入りうるのは `return` の節だけである
- `run/effects/`
  - `file_across_once_of_multi_effect.em`: `multi` の操作も持つエフェクトの `once` の操作をまたいで `File` を持ち、正しく動くことを確かめる

### テストの変更

| 種類 | 変更 |
|---|---|
| 1 | `tests/ui/run/effects/multi_over_once.em` を `tests/ui/check-fail/linearity/` に移し、冒頭のコメントを直す。スナップショットは `run/` の出力から E3006 の診断に変わる |
| 1 | `crates/eml_types/tests/effects.rs` の `the_return_clause_of_a_multi_handler_cannot_capture_a_linear_value` の期待値を、E3001 の文言から E3006 の文言にする |

`KindConstraint` はテストから表示でだけ使われているので、enum にしても、今の線形性の制約の表示とテストは変わらない。

種類1の2件は `test-changes.md` に記録する。既存の `run/` のテストや、上にない期待値が新しい規則で変わると分かったときは、その時点で作業を止めて相談する。精度の限界に当たるものが特に考えられる。
