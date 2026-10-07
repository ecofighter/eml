# S2a 継続を関数にする (設計)

位置づけ: 作業の設計。再設計のサブプロジェクト S2a の spec である。全体の方針は [ロードマップ](../../future/roadmap.md) の「S2a 継続を関数にする」と、[全体設計](2026-10-07-redesign-design.md) にある。決まったことは S2a の終わりに `docs/` の該当文書へ移し、この文書を削除する。

## 目的

handler の節の継続 `k` を、線形性を持つ第一級の普通の関数にする。`resume` キーワード、継続の専用の型 `Cont`、状態の欄の推論を消し、型検査と HIR の特別扱いを減らす。

## 背景

今の継続は、次の専用の仕組みで扱っている。

- 構文: `resume k v` と `resume k v s` は専用の式で、`resume` はキーワードである (`ExprKind::Resume`)
- 型: `k` の型は専用の `Cont` 型で、操作の結果の型、handle の結果の型、handle の外側の row、状態の欄を持つ (`TyShape::Cont`、書き出す型の `Type::Cont`)
- 状態の欄: 2引数の `resume` と3引数の `resume` のどちらが正しいかを決めるために、型とは別の推論の種類 (Slot / SlotVar) がある。食い違いは E2007 になる
- 引数の個数: `resume` の引数が2個でも3個でもなければ E1011 になる。E1011 は `drop` の引数の個数も見ている
- 持ち越し規則: `resume` の呼び出しの row を `CallRows::Resume` として特別に扱う
- S1 の `mask`: `resume` の式をキーにして記録している (`(resume の式, 0)`)

型検査が書き出す関数型 (`Type::Fn`) は、矢印の線形性を持たない。線形性は Kind を解いた後に決まるので、持たせると本体の型を Kind を解くまで確定できなくなるためである ([コンパイラの構成](../../implementation/architecture.md) の「`eml_types` の内部」)。そのため、`File` を捕まえた `Lin` のクロージャも、診断ではほかの関数と同じ形で表示されている。

## 決めたこと

### 構文と HIR

- `resume` をキーワードから外し、普通の名前にする。`resume` の式の構文、CST のノード、`ExprKind::Resume` を消す
- 再開は普通の関数適用で書く。状態のない handler では `k v`、状態のある handler では `k v st` と書く
- 節の書き方 (`| get () k st -> …`) と `drop k` は変えない
- E1011 は消さず、`drop` の引数の個数の誤りだけを見る診断にする

### 型

- 節の `k` の型は次の関数型である。`m` は、`once` の操作なら `Lin`、`multi` の操作なら `Unr` である。`a` は操作の結果の型、`r` は handle の結果の型、`<outer>` は handle の外側の row、σ は状態の型である

  | handler | `k` の型 |
  |---|---|
  | 状態なし | `a -[m]-> <outer> r` |
  | 状態あり (`from`) | `a -[m]-> σ -[m₂]-> <outer> r` |

- 状態ありの2つ目の矢印の線形性 `m₂` は、`m` と `a` の Kind の join である。`k v` と部分適用したクロージャは `v` を捕まえるので、`multi` の `k` でも `a` が `Lin` なら、そのクロージャは1回しか呼べない。コンストラクタやラムダの部分適用と同じ規則である。`once` の `k` では `m₂` は `Lin` になる

- row は最後の矢印に付ける。状態ありの `k v` は部分適用で、エフェクトを起こさないためである。最初の矢印の row は空の閉じた row である
- `Cont` 型 (`TyShape::Cont`、`Type::Cont`、`ContState`)、状態の欄 (Slot / SlotVar と `unify_slot`)、E2007 を消す。E2007 は欠番にし、ほかの診断に使わない
- `resume` の引数の個数の検査を消す。個数の誤りは、普通の関数適用の誤り (E2001 など) として報告する
- `CallRows::Resume` と、持ち越し規則の `resume` の特別扱いを消す。`k v` は普通の呼び出しとして、その矢印の row (handle の外側の row) で持ち越し規則にかかる
- S1 の `mask` は、`k v` が普通の呼び出しになるので、呼び出しの矢印ごとの記録 (`(呼び出しの式, 矢印の番号)`) で扱う。`resume` の式をキーにした記録はなくなる
- HIR は、節の `k` を、引数の数が分かる呼び出し先 (状態なしは1、状態ありは2) として扱う。評価の手順 (`eml_hir::call_steps`) は、引数の数の分からない局所変数の呼び出しを矢印ごとに区切るので、そのままでは `k st (st + 1)` が「`k st` の部分適用の後に `st + 1` を評価する」手順になるためである。最初の矢印の row は空で、部分適用は何も起こさないので、評価の順を観測できる形では変えない。評価の手順は持ち越し規則と translate が共有するので、両方に同じように効く
- `k` の型は、普通の関数と同じ形で表示し、線形性の印を付けない。ほかの `Lin` のクロージャの表示と揃えるためである。表面の構文で線形性を書けるようにするかと、その表示の印は、S5 の「Kind の宣言化」で一緒に決める
- 単一化の失敗が矢印の線形性の食い違いだけによるとき (`once` の `k` や線形な値を捕まえたクロージャを、`data` のフィールドのような `Unr` の矢印の位置に渡したとき)、書き出す型には線形性が出ないので、E2001 の期待した型と実際の型が同じ表示になる。このときは E2001 に、関数が1回しか呼べないのに、その位置は何度でも呼べる関数を求める、という趣旨の note を付ける
- `once` の `k` を2回使う、使わずに捨てる、といった誤りは、今と同じく E3002、E3003、E3005 が報告する。E3005 の文言は、「`resume` も `drop` もしなかった」を「呼びも `drop` もしなかった」にする

### 意味

- `k` は第一級の値である。変数に束縛し、関数に渡し、データにしまい、ラムダで捕まえてよい。`once` の `k` を入れたデータやクロージャは `Lin` になる。これは Kind の join と、`Lin` の値を捕まえたクロージャは `Lin` になるという今の規則から決まり、新しい規則は要らない
- handle が値を返した後で `k` を再開してもよい。deep handler では、捕まえた区間の中の handler フレームが、再開した先の継続の上にもう一度つながる。状態のある handler では、`k v st` の `st` がその handler フレームの状態になる
- `k` を構文で節の中に閉じ込める (second-class にする) 案は採らない。async やスケジューラの handler は `k` を実行待ちの列にしまって後で再開するので、閉じ込めると並行処理が書けなくなる ([マルチコア対応の設計](../../future/multicore.md) の `spawn` と `run_scheduler`)。閉じ込めなくても、`k` を値として使うかどうかは節の本体を見れば分かるので、下の「Core IR」のとおり、値として使わない場合は確保なしにコンパイルできる

### Core IR

- translate は、節ごとに `k` の使い方を HIR で調べ、次の2つの形のどちらかに変換する
  - 直接の形: 節の本体の中のどこでも (入れ子のラムダや handle の本体の中を含めて)、`k` の使用がすべて、引数をそろえた直接の呼び出し (`k v`、状態ありなら `k v st`) か `drop k` なら、クロージャを作らない。生の継続をラムダや handle の本体が捕まえても、`Call::Resume` で再開できるためである。その呼び出しを、生の継続への `Call::Resume` にする。S1 の `mask` は、その呼び出しの最後の矢印の記録を付ける。引数が余る呼び出し (`r` が関数型のときの `k v x`) は、`Call::Resume` の結果への `apply` を続ける
  - 包む形: `k` を関数の値として使う (渡す、しまう、部分適用する) なら、節の入口で、生の継続を「継続を包む関数」のクロージャにし、それを `k` とする。`k` の呼び出しは普通の関数値の呼び出し (`Call::Apply`) になる
- 継続を包む関数は、状態なし用 (`k v` を受けて `resume k(v, ())` を末尾呼び出しする) と状態あり用 (`k v st` を受けて `resume k(v, st)` を末尾呼び出しする) の2つで、使うときだけプログラムに1つずつ作る。名前は、操作やコンストラクタを包む関数 (`op$…`、`con$…`) の付け方に合わせて計画で決める
- `Call::Resume` は、Core IR の内部の命令として残る。そのため、インタプリタ、ランタイム、Perceus、verifier は変えない
- `once` の `k` は1回しか使わないので、Perceus は `dup` / `decref` を付けない。包んだクロージャも一意で、呼ぶと消費される
- 包んだ `k` を `drop` すると、クロージャが解放され、それが継続を解放する。継続の区間が捕まえていた `Lin` の値は、今と同じく区間の解放で後始末される

### ネイティブ化とマルチコアとの関係

- evidence passing では、継続は yield の bubbling で作るヒープのクロージャである。直接の形の `Call::Resume` は、その継続の関数を呼ぶことに当たる。包む形のクロージャは、その継続を値として持ち回るときの呼び出し規約に当たる。末尾で1回だけ再開する節は、evidence passing の段で継続そのものを作らない形にできる ([evidence passing の設計](../../future/evidence-passing.md) の「すぐに再開する節」)
- ネイティブでは、包んだ `k` を `drop` したとき、継続が捕まえている `Lin` の値の破棄処理を呼ぶ必要がある。ネイティブのオブジェクトは型の情報を持たないので、オブジェクトの形が「破棄処理を持つ欄」を表せなければならない。これを S3b への申し送りとして、ロードマップの S3b の論点に書く
- マルチコアでは、`once` の `k` は一意な `Lin` の値なので、ほかのスレッドへ移せる。`multi` の `k` が捕まえているのは持ち越し規則により `Unr` の値だけなので、共有しても安全である。包むクロージャは中の継続の性質を引き継ぐだけで、新しい制約を持ち込まない ([マルチコア対応の設計](../../future/multicore.md) の「継続の移動と不変条件」)

## 対象外

- 末尾で1回だけ再開する節を、継続を作らずにその場で実行する最適化 (evidence passing の段)
- 表面の構文で矢印の線形性を書くことと、その表示 (S5)
- 組み込みの extern 化 (S2b)

## テスト

### 足すテスト

UI テスト (`tests/ui/`):

| テスト | 確かめること |
|---|---|
| `run/effects/continuation_passed.em` | `k` を補助関数に渡し、そこで再開する (包む形) |
| `run/effects/continuation_stored.em` | `multi` の操作の `k` をデータにしまい、handle が値を返した後で取り出して再開する (handler フレームがもう一度つながる)。`once` の `k` は `Lin` なので、矢印が `Unr` に決まる `data` のフィールドにはしまえない |
| `run/effects/continuation_partial.em` | 状態のある handler の `k v` を部分適用して渡し、渡した先で状態を与えて再開する |
| `run/effects/continuation_multi_passed.em` | `multi` の `k` を包む形で2回呼ぶ |
| `check-fail/linearity/continuation_in_lambda.em` | `once` の `k` を捕まえたラムダを2回呼ぶと E3002 になる |
| `check-fail/types/linear_function_in_unr_field.em` | `once` の `k` を `data` のフィールドにしまうと、線形性の食い違いを説明する note 付きの E2001 になる |
| `run/effects/continuation_state_expression.em` | 状態のある handler で `k st (st + 1)` のように引数に式を書いても1回の再開になる |

段階ごとのテスト:

| crate | 確かめること |
|---|---|
| `eml_syntax` | `resume` が普通の名前として読めること |
| `eml_types` | 節の `k` の型 (状態なし、状態あり、`once` と `multi` の矢印の線形性、状態ありの2つ目の矢印が `a` の Kind を受けること)、`k` の引数の個数の誤りが関数適用の誤りになること、`k v` の呼び出しに `mask` が記録されること |
| `eml_hir` | 節の `k` の呼び出しの評価の手順が、引数の数の分かる呼び出し先と同じになること |
| `eml_core_ir` | translate が直接の形と包む形を正しく選ぶこと (直接の呼び出しだけ、`drop k`、入れ子のラムダや handle の本体の中の直接の呼び出し、渡す、部分適用)、`k st (st + 1)` が1つの `resume` になること、継続を包む関数が使うときだけ作られること |

### テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。

- 成否の変更: なしを見込む。書き換えた結果、check-fail の UI テストが誤りにならなくなった場合 (状態のある handler の `k v` が部分適用として通る、など) は、実装を止めて確かめる
- 期待値の変更:
  - `resume` を書いた UI テスト (約50本) の本文を、`k v` / `k v st` に書き換える。run テストの出力は変えない。check-fail テストは、診断の文言、番号、位置が変わってよい
  - E2007 と `resume` の引数の個数を確かめる UI テスト (`check-fail/types/resume_state.em`、`check-fail/names/resume_and_drop_arity.em`、`check-fail/names/handler_state_arity.em`) は、新しい形で関数適用の誤り (または `drop` の E1011) になることを確かめる形に書き直す。誤りにならなくなるケースは、そのケースだけを消す (成否の変更にあたらないよう、ファイルは check-fail に残る誤りを1つ以上持つ)
  - E3005 の文言が変わる UI テストと段階ごとのテスト
  - `check-fail/syntax/unparenthesized_lambda.em` の `inc resume k 1` の行は、`resume` が普通の名前になって E0012 にならないので、`drop` の行に置き換える
  - `Cont` の表示名がなくなるので、ユーザーが定義した `data Cont` の表示が `Main.Cont` から `Cont` に変わる (`eml_hir` の `def_map.rs`、`eml_types` の `modules.rs` のテスト)
  - 状態のある handler で部分適用の結果の型が型変数のままだと、誤りが E2001 ではなく E2005 になる場合がある。書き換えた check-fail テストの期待値として受け入れる
  - 各 crate の `resume` の構文 (`eml_syntax`)、HIR (`eml_hir`)、型 (`eml_types` の `Cont` の表示、E2007、状態の欄)、Core IR (`eml_core_ir`、`eml_interp`) のテストを、新しい形に書き換えるか、対象がなくなったものを消す。`Cont` を表示するダンプは関数型の表示に変わる。Core IR のダンプは、直接の形なら今と同じ `resume`、包む形なら継続を包む関数とクロージャが入る
- 機械的な追随: `ExprKind::Resume` と `Cont` 型を消すことに伴うパターンの書き換え

## 更新する文書

S2a の終わりに、次の文書を直す。

| 文書 | 直すこと |
|---|---|
| `docs/spec/effects.md` | `k` を関数として書き直す (「継続の多重度と持ち越し規則」「handler の意味」)。「パラメータ付き handler」の状態の欄の説明を消し、`k v st` にする。handle が値を返した後の再開の意味を書く |
| `docs/spec/expressions.md` | handler の節と再開の書き方を `k v` / `k v st` にし、`resume` の構文と E1011 の `resume` の部分を消す |
| `docs/spec/grammar.md`、`docs/spec/lexical.md` | `resume` の式とキーワードを消す |
| `docs/spec/types.md` | 継続の型が普通の関数型であることを書く (関数型と推論) |
| `docs/spec/linearity.md` | 継続の線形性の記述を、`k` の矢印の線形性として書き直す |
| `docs/spec/diagnostics.md` | E2007 を欠番にし、E1011 を `drop` だけにし、E3005 の意味を直す |
| `docs/spec/core-ir.md` | 直接の形と包む形、継続を包む関数を書く。`Call::Resume` が内部の命令であることを書く |
| `docs/spec/examples.md` | `resume` を書いた例を書き換える |
| `docs/implementation/architecture.md` | 状態の欄と `Cont` の記述を消し、translate の2つの形を書く |
| `docs/implementation/diagnostics.md` | E2007 の行を消し、E1011 と E3005 の行を直す |
| `docs/implementation/status.md` | 実装したものの `resume` を `k` の呼び出しにする |
| `docs/future/roadmap.md` | 「S2a 継続を関数にする」の節と段の列の行を消す。S3b の論点に、破棄処理を持つ欄の申し送りを足す |
| `docs/overview.md` | 継続の行の「(S2a で入れる)」を外し、パラメータ付き handler の行を合わせる |
| `docs/spec/modules.md`、`docs/future/*.md`、`README.md` | `resume` を書いた例と説明を `k` の呼び出しにする |

## 完了の条件

- `Cont` 型、状態の欄、E2007、`ExprKind::Resume`、`CallRows::Resume`、持ち越し規則の `resume` の特別扱いが消える
- `resume` を書いた UI テストを書き換えたうえで、run テストの出力が変わらない
- 上の足すテストがすべてあり、`cargo test` がすべて通る
- 上の文書を更新し、引用の検査が通る
