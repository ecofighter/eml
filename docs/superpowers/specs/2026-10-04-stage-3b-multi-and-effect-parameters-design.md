# 縦の貫通 段階3b: `multi` とエフェクトの型引数の設計

位置づけ: 作業用の設計文書。段階3b を終えたら、残す価値のある内容を `docs/spec/`、`docs/implementation/architecture.md`、`docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

[status.md](../../implementation/status.md) の「名前解決以降の実装段階」の段階3b (`multi` と multi-shot の再開、エフェクトの型引数) を、HIR、型検査、Core IR、ランタイム、インタプリタの全体に通す。

### 3b に含めるもの

- `multi` の操作と multi-shot の再開。同じ `k` を2回以上 `resume` すること、`drop k` すること、使わないことを含む
- エフェクトの型引数。宣言 `effect State s where ...` と、row の中の `<State Int | e>`
- 局所的に決まる静的な規則を2つ足す。`multi` の操作の `k` は `Unr` の継続にする。扱うエフェクトに `multi` の操作がある handle では、`return` の節が捕まえる変数に `Unr` の制約を付ける

### 段階5に残すもの

- 持ち越し規則。`multi` の呼び出しをまたいで生きている `once` の `k` は、区間を写すときに複製される。この穴は status.md に記録する
- 外側の `multi` の handler が、内側の handler フレームを含む区間を写すと、内側の `return` の節が2回動く。`return` の節が捕まえた値は、本体の呼び出しをまたいで生きている値に当たるので、これも持ち越し規則で扱う

### 範囲の外

- row を引数に取るエフェクト (`effect Spawn e`)。エフェクトの引数は型だけにする
- エフェクトの引数に型の適用を書くこと (`<State (List Int)>`)。型の適用は段階4で実装するので、今の E0004 のまま残す
- `from` によるパラメータ付き handler (段階6)。引き続き E0004 にする

### ユーザーと合意済みの決定

- 持ち越し規則は段階5に残す (案A)。ランタイムは、共有された継続を再開するときに区間を必ず写す。そのため、`once` の `k` が複製されてもメモリ安全で、`debug_heap` も通る。持ち越し規則のうち `multi` に関わる部分を 3b に前倒しする案 (案B) は採らない。案B には、呼び出しをまたいで生きている変数の情報が要る。さらに、生きている変数の Kind が Kind 変数 `μ` で、row が row 変数 `e : Row<σ>` のとき、「`μ = Lin` なら `σ ≤ Once`」という条件つきの制約になり、今の束の不等式では書けない
- エフェクトの引数の Kind は固定しない。handle ごとに具体的な型で検査する
- フレームはつねに一意に保ち、共有されうるのは継続オブジェクトだけにする

## 1. HIR と名前解決

### item

- `EffectDef` に `generics: Generics` を持たせ、宣言の型引数を `type_vars` に入れる。
- 各操作の `Generics` は、エフェクトの型引数を先頭に写して始める。操作のシグネチャに同じ名前の型変数が現れたら、エフェクトの引数を指す。それ以外の型変数は、今までどおり操作ごとに暗黙に量化する。
- `Operation` に、`Generics` の先頭の何個がエフェクトの引数かを持たせる (`effect_params: usize`)。アリーナを分けずに済み、型検査はエフェクトの引数と操作自身の型変数を番号で区別できる。
- `OpMultiplicity::Multi` を足し、`multi` の E0004 を外す。エフェクトの型引数の E0004 も外す。
- 宣言の型引数の名前の重複 (`effect P a a`) は E1003 にする。

### row

`RowRef` のエフェクトの並びを、型引数を持つ形にする。

```rust
pub struct EffectRef {
    pub effect: EffectId,
    pub args: Vec<TypeRefId>,
}

pub enum RowRef {
    Omitted,
    Closed { effects: Vec<EffectRef>, range: TextRange },
    Open { effects: Vec<EffectRef>, tail: RowVarId, range: TextRange },
    Error,
}
```

- 型引数は `type_atom` なので、`Int`、型変数、括弧で囲んだ関数型を書ける。型の適用は今の E0004 のままである。
- 型引数の個数が宣言と違えば E1015 を出し、その row を `RowRef::Error` にする。型検査で診断を連鎖させないためである。

### E1008 の範囲

E1008 は「`never` の操作の結果の型が、引数に現れない**操作自身の**型変数でない」に絞る。`never raise : Unit -> s` で `s` がエフェクトの引数なら、呼び出した側は結果を自由な型として使えないので、誤りになる。

### 診断

| 番号 | 定数 | 内容 |
|---|---|---|
| E1015 | `TYPE_ARGUMENT_COUNT` | row の中のエフェクトの型引数の個数が、宣言と違う (`<State>`、`<State Int Int>`)。段階4の `data` の型引数でも使う |

## 2. 型検査

### row のラベル

- 型の表の `Row::labels` を `Vec<Label>` にする。

```rust
pub(crate) struct Label {
    pub effect: EffectId,
    pub args: Vec<Ty>,
}
```

- 書き出す `eml_types::EffectLabel` にも `args: Vec<Type>` を足す。診断では `<State Int | e>` と表示し、関数型の引数は括弧で囲む。
- scoped labels の単一化では、今と同じくエフェクトの ID でラベルの対を作る。同じエフェクトのラベルが複数あるときは、それぞれの row の中の順で i 番目どうしを対にする。そのうえで、対にしたラベルの型引数を単一化する。
- 型引数が一致しなければ、`UnifyError` に新しい種類 (`EffectArgs`) を足し、E2001 として報告する。メッセージは「`State String` を起こしたが、row では `State Int` である」のような形で、呼び出しを primary にする。エフェクトそのものは row にあり、引数だけが違うので、E2002 (row に含まれない) とは区別する。
- `bind_row` が出す多重度の下限 (`effect_multiplicity`) は、ラベルのエフェクトの ID から引く。型引数は多重度に関わらない。

### 操作のスキーム

- 操作のスキームは、エフェクトの引数と操作自身の型変数の両方で量化する。最後の矢印に付ける閉じた row は `<E a1 .. an>` で、`ai` はエフェクトの引数の変数である。
- 呼び出しでは、これまでどおりすべての変数を新しい変数で具体化する。`get ()` の結果の型は、row の単一化を通じて、今の row の `State Int` から決まる。
- 操作の引数の型の Kind 変数を `Unr` に固定する 3a の規則 (`Table::unrestricted`) から、エフェクトの引数を外す。エフェクトの引数は handle ごとに具体的な型で検査されるので、節での使い方がその型の Kind に普通に伝わるためである。そのため `State File` のような線形な状態も、規則の上では書ける。固定するのは、操作自身の型変数と、引数の型の中の矢印の線形性だけのままにする。

### handle の検査

handle 式の期待する型を `τ`、その位置の row を `ρ`、扱うエフェクトを `E` とする。

- 本体の row は `<E α1 .. αn | ρ>` である。`αi` は handle ごとの新しい推論用の変数である。
- 操作の節では、エフェクトの引数を `αi` で、操作自身の型変数を節だけの rigid な変数で具体化する。3a の「操作の型変数は節の中で rigid」の規則は、操作自身の型変数だけに当てはまることになる。
- 本体がそのエフェクトを起こさず `αi` が決まらなくても、誤りにはしない。型は消去されるので、実行には関わらない。

### `multi`

- `multi` の操作の節の `k` は `Cont R τ ρ` で、線形性は `Unr` である。`resume` を何回書いても、`drop k` しても、使わなくてもよい。
- `Table::effect_multiplicity` は今も操作の多重度の最大を返す。そのため、`multi` の操作を含むエフェクトはそのまま `Multi` になる。
- row 変数の `σ` には下限だけが出る。上限は持ち越し規則 (段階5) で出るので、上限のテストは段階5に残す。

### 使用回数のパス

- 扱うエフェクトに `multi` の操作がある handle では、`return` の節が捕まえる変数に、使用の回数によらず `Unr` の制約を付ける。`k` を2回再開すると、handler フレームを含む区間が写され、`return` の節のクロージャが2回呼ばれるためである。
- 由来の理由に `KindReason::CapturedByReturnClause` を足し、E3001 のラベルにする。
- handle の本体の捕獲は今のまま (handle 式の位置での1回の使用) にする。本体のクロージャ自体は1回しか呼ばれない。`perform` の後に何度も動く部分の変数はフレームに退避されていて、その複製を静的に止めるのは持ち越し規則 (段階5) である。

## 3. Core IR

- エフェクトの型引数は型の消去で消えるので、Core IR には変更がない。
- `multi` の操作は、`Program` のエフェクトの表で `resumable: true` になる。`once` との違いは静的な検査だけで、命令は同じである。
- `k` を2回使う節には、Perceus が今の規則のまま `dup` を付ける。`VarInfo::linearity` はつねに `Unr` なので、新しい規則は要らない。

## 4. ランタイムとインタプリタ

### 不変条件

フレームはつねに一意である。共有されうるのは継続オブジェクトだけで、継続オブジェクトが区間の先頭のフレームを所有する。今の `ret` が `take` でフレームを取り出す前提は、3b でもそのまま成り立つ。

### 区間の複製

`Heap::take_or_copy` が継続オブジェクトを受け取ったときは、次のように扱う。

- 一意なら、今と同じく取り出す。
- 共有されていれば、`top` から、切り離された handler フレーム (`next` が `None`) までの区間を、先頭から順に写す。写したフレームは、`next` 以外の子 (退避した値、`Apply` の引数、節のクロージャ、`return` の節) の参照を1つずつ増やし、`next` は写した次のフレームを指す。最後に、写した区間の先頭と handler フレームを持つ新しい `Payload::Continuation` を返し、元の継続オブジェクトの参照を1つ手放す。
- 元の区間のフレームの参照の数は変えない。そのため、写した後も、両方の区間のフレームは一意のままである。
- 区間をたどる処理は、解放と同じく再帰ではなくループにする。長い区間で Rust のスタックがあふれないようにするためである。
- 区間の中にある、外側につながったままの handler フレーム (本体の中の別の handle) も、普通のフレームとして写す。
- 今の `copy` の `Payload::Continuation` の扱い (参照をそのまま写すだけ) は、`top` を共有させて不変条件を壊すので、この処理に置き換える。

### インタプリタ

- `resume` は、継続オブジェクトの取り出しを `heap.take` から `heap.take_or_copy` に替える。その後の「handler フレームの `next` に今の継続を入れる」処理は今のままにする。写した区間の handler フレームは一意なので、書き換えてよい。
- `drop k` は今と同じく `decref` する。共有されていれば参照が減るだけである。

### 計算量

一意な継続の `resume` は、今と同じく O(1) である。共有された継続の `resume` は、区間の長さに比例する。最後の1回の `resume` (例えば `resume k 1 + resume k 2` の2回目) は一意なので、写さずに済む。

### 3b の後も残る穴

`multi` の区間を写すと、フレームが退避していた `once` の `k` は参照が増えるだけである。その `k` を2回再開すると、2回目は上の「共有されていれば」の扱いで区間が写される。そのため、メモリ安全で `debug_heap` も通る。ただし、`once` の継続が2回動く。これは段階5の持ち越し規則で静的に止める。

## 5. テスト、文書、成功の条件

### 変わるテスト

| テスト | 種類 | 変更 |
|---|---|---|
| `tests/ui/check-fail/later_stage_effects.em` とスナップショット | 1 | `multi` とエフェクトの型引数の部分を除き、`from` の E0004 だけを残す |
| `crates/eml_hir/tests/effects.rs` の操作の宣言の診断のテスト | 1 | 期待値から `E0004 6:3 \`multi\` operations are not supported yet` の行を除く |
| `crates/eml_hir/tests/effects.rs` の `effect_type_parameters_and_arguments_come_in_stage_3b` | 1 | E0004 を期待するテストをやめ、型引数が HIR に入ることを確かめるテストに置き換える |
| row とラベルを組み立てるテスト (`eml_types` の `table/tests.rs`、`ty.rs`) | 3 | ラベルを `Label` と `EffectLabel { args: vec![] }` で書く。期待値は変えない |
| `EffectDef` と `Operation` を組み立てるテスト | 3 | `generics` と `effect_params` を足す。期待値は変えない |
| `RowRef` を組み立てるテストや、網羅的な `match` を書いたテスト (`OpMultiplicity` など) | 3 | 新しい形と種類に追随する。期待値は変えない |

種類1の変更は `testing.md` に記録する。上の表にないテストの期待値が変わった場合は、変えずに止まり、差分と理由をユーザーに示して承認を得る。

### 足すテスト

UI テスト `run/` (どれも `debug_heap` が有効)

| ファイル | 確かめること |
|---|---|
| `multi_resume.em` | 末尾でない `resume` を2回書く (`resume k 1 + resume k 2`)。末尾の `resume` との組み合わせ。`multi` の `k` の `drop` と未使用 |
| `multi_choice.em` | `choose` を3回起こし、すべての経路を数える。写す区間に、文字列を退避したフレームと、外側につながった handler フレームを含める |
| `multi_captured.em` | `multi` の `k` をクロージャで捕まえて、後で何度も再開する。`k` を関数に渡して再開する |
| `effect_parameters.em` | 関数を返す handler で `State Int` を実装する。`Reader String`。同じエフェクトを違う引数で入れ子にする (内側が処理する)。`<Reader r \| e>` に多相な関数 |
| `multi_loop.em` | 1万回程度の共有された `resume` で、写した区間がリークなく解放される |

UI テスト `check-fail/`

| ファイル | 確かめること |
|---|---|
| `effect_arguments.em` | E1015 (少ない、多い)、型引数の不一致の E2001、エフェクトの引数を結果にした `never` の操作の E1008、宣言の型引数の重複の E1003 |
| `multi_return_clause.em` | `multi` の操作を持つ handler の `return` の節が、外側の `once` の `k` を捕まえたときの E3001 |

crate ごとのテスト

- `eml_hir`: エフェクトの `Generics` と HIR の表示。操作の `Generics` の先頭がエフェクトの引数を指すこと。E1015
- `eml_types`: 型引数つきのラベルの単一化 (同じエフェクトのラベルが複数あるときの順の対)。`EffectArgs` の誤り。操作のスキームの row。`unrestricted` がエフェクトの引数を固定しないこと。`multi` の `k` が `Unr` であること。`<State Int | e>` の表示
- `eml_runtime`: 共有された継続の `take_or_copy` が区間を写し、元のフレームの参照の数を変えないこと。外側につながった handler フレームも写すこと。両方の区間がリークなく解放されること

### レビューで重点的に見るところ

- 区間を写した後も、どのフレームも一意であること (`ret` の `take` が失敗しない)
- 写した区間と元の区間で、退避した値の参照がちょうど1回ずつ数えられること (`multi_choice.em`、`multi_loop.em`)
- 継続オブジェクトが所有しない `handler` の参照が、写した区間では写した handler フレームを指すこと

### 文書

| 文書 | 変更 |
|---|---|
| `docs/spec/effects.md` | エフェクトの引数の意味 (Kind を固定しない、handle ごとに具体化する)。`return` の節の `Unr` の規則。multi-shot の再開 |
| `docs/spec/declarations.md` | エフェクトの型引数、操作のシグネチャでの名前の扱い、E1008 の範囲 |
| `docs/spec/types.md` | row のラベルの型引数と単一化 |
| `docs/spec/linearity.md` | `return` の節の規則。操作の引数の Kind の規則からエフェクトの引数を外すこと |
| `docs/spec/core-ir.md` | 共有された継続の再開で区間を写すこと |
| `docs/spec/runtime.md` | 区間の複製と、「フレームはつねに一意」の不変条件 |
| `docs/spec/diagnostics.md` | E1015 と、型引数の不一致の E2001 |
| `docs/implementation/architecture.md` | ラベルの表現、エフェクトの `Generics`、区間の複製 |
| `docs/implementation/status.md` | 3b を完了にする。3b の注意点を整理し、`once` の `k` が複製される穴と、外側の `multi` で `return` の節が2回動く件を段階5の項目として残す。「完了した作業」に 3b の行を足す |
| `docs/implementation/testing.md` | 種類1の変更を記録する |

### 成功の条件

- `multi` の操作とエフェクトの型引数が E0004 にならず、上の UI テストが通る。`from` は E0004 のままである
- multi-shot の再開が `debug_heap` を有効にして動き、リークも解放済みアクセスもない
- E1015、型引数の不一致の E2001、`return` の節の E3001 が上の UI テストで報告される
- 変わったテストは上の表のものだけである
- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` が通る
- 上の「文書」の表の変更が済んでいる
