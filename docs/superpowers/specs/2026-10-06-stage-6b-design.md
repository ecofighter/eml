# 段階6b: パラメータ付き handler の設計

位置づけ: 作業用の設計文書。段階6b を終えたら、残す価値のある内容を `docs/spec/`、`docs/implementation/architecture.md`、`docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

`handle ... from ... with` で書くパラメータ付き handler を、脱糖せずに状態を handler フレームに持つ形で、HIR からインタプリタまで通す。言語の意味は spec で決まっている ([エフェクトと handler](../../spec/effects.md) と [式](../../spec/expressions.md) の「パラメータ付き handler」、[Core IR とインタプリタ](../../spec/core-ir.md)、[線形性](../../spec/linearity.md) の「パラメータ付き handler の状態」)。この文書は、spec が決めていない点と、実装の形を決める。

段階6b は2回に分け、6b-1 → 6b-2 の順に、それぞれ計画と実装のサイクルで進める。spec はこの文書1つにまとめる。

| 回 | 内容 | 観測できる振る舞い |
|---|---|---|
| 6b-1 | handler の形の作り替え。HIR の `Closure`、Core IR の `Handle` と `Resume` がつねに状態と `return` の節を持つ形、`Atom::Fn`、`perform` の多重度、ランタイムの `Link` (1章) | 変えない。UI テストの出力は1バイトも変わらない |
| 6b-2 | `from` の機能。HIR の初期値と3引数の `resume`、継続の型の状態の欄、E2007 と fix、省いた `return` の節の E3004、状態の持ち越し (2章) | 変える。`from` と3引数の `resume` が E0004 でなくなる |

6b-1 で状態の通り道をすべて作っておき、6b-2 では、状態の値が `()` 以外になる経路と、型検査と診断を足すだけにする。

## 0. spec の穴について決めたこと

| # | 穴 | 決めたこと | 載せる文書 |
|---|---|---|---|
| D1 | 状態の欄の誤りの番号 | E2007 `RESUME_STATE_MISMATCH` (2.4) | diagnostics.md |
| D2 | `return` の節を省いた handler の状態が `Lin` のときの診断 | E3004 を広げる。primary は `from` の初期値の式で、help で `return` の節を書くよう伝える。fix は付けない (2.3) | diagnostics.md、expressions.md |
| D3 | E2007 の fix | 3つ目の引数を除く fix は常に付ける。状態を足す fix は、条件を満たすときだけ節の状態の変数を渡す (2.4) | diagnostics.md |
| D4 | fix の形 | `Diagnostic::fix` を `Option<Fix { title, edits }>` にする。1つの診断が持つ fix は高々1つなので、`Vec` にはしない (2.4) | diagnostics.md |
| D5 | 状態の持ち越しでまたぐ row | handle の外側の row。扱うエフェクトの操作が起きると状態は節に渡るので、本体の row 全体ではない (2.3) | effects.md、linearity.md |
| D6 | 状態のある handler の E1010 と、E1011 の言い方 | 2.1 の文言にする | なし (診断の文言) |

D5 の理由をもう少し書く。本体の実行中、状態は handler フレームの `Link` にある。扱うエフェクトの操作が起きると、`perform` は状態をフレームから取り出して節に渡すので、捕まえた区間に状態は入らない。外側のエフェクトの操作で区間が捕まると、自分の handler フレームは状態を持ったまま区間に入り、`multi` なら写される。そのため、状態は外側の row の `multi` だけを気にすればよい。spec の「handle の外側の row に `multi` の操作があれば、状態は `Unr` でなければならない」(effects.md) と同じ結論である。

## 1. 6b-1 handler の形の作り替え

### 1.1 HIR: `Closure`

ラムダ、handle の本体、操作の節、`return` の節は、どれも「引数のパターンの並びと本体」で、Core IR では捕まえた変数を先頭の引数に持つ関数に持ち上げる。これを1つの形にする。

```rust
pub struct Closure {
    pub params: Vec<PatId>,
    pub body: ExprId,
}

ExprKind::Lambda(Closure)
ExprKind::Handle {
    /// 引数のない closure。Core IR では `()` を受ける関数になる。
    body: Closure,
    effect: Option<EffectId>,
    clauses: Vec<OpClause>,
    ret: ReturnClause,
}

pub struct OpClause {
    pub op: OperationId,
    /// 操作の引数、`k` (`never` 以外)、状態 (6b-2 の状態のある handler だけ) の順。
    pub closure: Closure,
    pub range: TextRange,
}

pub struct ReturnClause {
    /// 本体の値、状態 (6b-2 の状態のある handler だけ) の順。
    pub closure: Closure,
    pub source: ClauseSource,
    pub range: TextRange,
}

pub enum ClauseSource {
    Written,
    /// HIR が合成した節。`range` は handle のキーワードを指す。
    Omitted,
}
```

- `OpClause` の `params` と `k` は `closure.params` にまとめる。`k` は操作の引数の数の位置にあるので、`OpClause::k(&self, module)` などの読み口で取り出す。
- 省いた `return` の節は HIR が `| return $r -> $r` として合成する。`$r` は等式の脱糖と同じ隠れた変数である。`Handle::ret` は `Option` でなくなる。
- 捕まえる変数は `Body::closure_captures(&Closure) -> Vec<LocalId>` の1つの関数で求める。今の `lambda_captures` と `captures(root, bound)` はこれに置き換える。Core IR の変換は closure ごとに1回だけ呼び、持ち越しのパスも同じ関数を使う。
- HIR のダンプ (`pretty`) は、合成した `return` の節も表示する。

### 1.2 Core IR

```rust
Call::Handle {
    effect: u32,
    init: Atom,
    body: Atom,
    clauses: Vec<Atom>,
    ret: Atom,
}
Call::Resume { k: Atom, arg: Atom, state: Atom }
Call::Perform { effect: u32, op: u32, resumable: bool, args: Vec<Atom> }
Atom::Fn(FnIdx)
```

- 状態のない handler では、`init` を `Atom::Unit` にし、2引数の `resume k v` を `state: Atom::Unit` にする (core-ir.md)。
- 節の関数は、最後の引数で状態を受ける。状態のない handler では、boxed でない新しい変数が `()` を受ける。`ret` の関数は、本体の値と状態を受ける。合成した `return` の節も、ほかの節と同じ経路で関数になる。
- `Atom::Fn` は、捕まえた変数のない関数を、クロージャを確保せずに値として使う形である。捕獲のないラムダ、handler の部分、`op$…`、`con$…`、`builtin$…` の包む関数を値として使う位置は、すべてこの形になる。boxed でないので、RC の対象にならない。
- `Perform::resumable` は、操作が `never` でないときに真である。変換がエフェクトの表から埋め、インタプリタはエフェクトの表を引かずに、区間を継続オブジェクトにするかその場で解放するかを決める。verifier は、`resumable` がエフェクトの表と一致することを確かめる。
- verifier は、`handle` の節の関数の引数の数が「捕まえた変数 + 操作の引数 + `k` (`resumable` のとき) + 状態」であること、`ret` の関数の引数の数が「捕まえた変数 + 2」であることを確かめる。`Atom::Fn` の場合は捕まえた変数が0個である。

テキストの形は次のとおり。

- `handle Ask(c1, init) {ask: c2} return c3`。`init` は2つ目の引数で、`return` はつねに書く。
- `resume k1(v2, s3)`。
- `Atom::Fn` は `&` に関数の名前を続けて書く (`&lambda$3`)。
- `perform` の表示は変えない。`parse` は、先頭のエフェクトの行の `never` から `resumable` を埋める。
- verifier が節の関数の引数の数を確かめるために、エフェクトの表の操作 (`OperationInfo`) に引数の数 `arity` を持たせる。先頭のエフェクトの行は、操作の名前の後に `/` と引数の数を書く (`effect Ask { ask/1, never stop/1 }`)。

### 1.3 ランタイムとインタプリタ

```rust
Frame::Handler {
    effect: u32,
    clauses: Vec<Value>,
    ret: Value,
    /// `None` なのは、継続に捕まえられて handle の外側から切り離されている間である。
    link: Option<Link>,
}

pub struct Link {
    pub next: ObjRef,
    pub state: Value,
}

Value::Fn(u32)
```

- つながっているのに状態がない形と、切り離されているのに状態がある形は、型として表せなくなる。
- `handle` は、`Link { next: 今の継続, state: init }` を持つフレームを積む。
- `perform` は、handler フレームの `link` を取り出して `None` にし、`next` を機械の継続に戻して、節に `state` を最後の引数で渡す。
- `resume k v s` は、区間の handler フレームの `link` を `Some(Link { next: 今の継続, state: s })` にしてから、区間の先頭に `v` を返す。区間を写す場合は、写した handler フレームに入れる。
- 本体の値が handler フレームに届いたら、`link` を取り出して `next` に戻り、`ret` に値と `state` を渡す。
- `Link::state` はフレームの子である。写すときは `dup` し、解放するときは `decref` する。外側のエフェクトの区間に入った handler フレームの状態は、この規則で写され、解放される。
- `Value::Fn` の呼び出しは、捕まえた値のないクロージャと同じ eval/apply で行う。引数が足りなければ、関数と渡された引数を持つクロージャを作る。

### 1.4 テストの変更

| 種類 | テスト | 理由 |
|---|---|---|
| 2 | `eml_core_ir` のスナップショットのうち、`handle`、`resume`、捕獲のないラムダ、包む関数の値を含むもの | `init`、`return`、`state`、`&f` の表示が増える。計画で実行して洗い出す |
| 2 | `eml_core_ir` の verifier と `eml_interp` のテキストの IR のうち、`handle` と `resume` を含むもの | テキストの形が変わる。期待する結果 (verifier の誤り、実行の出力) は変えない |
| 2 | ユーザーのエフェクトを持つ Core IR のスナップショットと、手で書いた IR の先頭の `effect` の行 | 操作の引数の数 (`/1`) が増える |
| 2 | `eml_hir` のダンプのうち、`return` の節を省いた handler を含むもの | 合成した節が表示に現れる |
| 3 | `eml_runtime` のテストのうち、`Frame::Handler` を組み立てるもの | 組み立てだけを `Link` に書き換え、期待値は変えない |

種類1の変更はない。UI テストのスナップショットは変わらない。

## 2. 6b-2 `from` の機能

### 2.1 HIR

- `ExprKind::Handle` に `init: Option<ExprId>` を足す。状態のない handler は `None` のままにし、型検査が状態のない handler と `from ()` を区別できるようにする。`from` の E0004 をやめ、本体と節を変換して名前の誤りを報告する。
- `ExprKind::Resume { k, arg, state: Option<ExprId> }` にする。3引数の E0004 をやめる。
- 状態のある handler で `return` の節を省いたら、`| return $r _ -> $r` を合成する。合成した `_` のパターンの範囲は `from` の初期値の式で、`ReturnClause::source` は `Omitted` である。
- 節の引数の数を、状態のある handler では1つ増やして確かめる。数が合わなければ E1010 にする。文言は次のとおり。

| 場合 | メッセージ | note |
|---|---|---|
| 状態のある handler の操作の節 | ``the clause for `ask` takes 3 parameters, but this one has 2`` | ``the clause takes the arguments of `ask`, the continuation `k`, and then the state`` |
| 状態のある handler の `never` の操作の節 | ``the clause for `fail` takes 2 parameters, but this one has 1`` | ```fail` is a `never` operation, so its clause takes the arguments of the operation and then the state`` |
| 状態のある handler の `return` の節 | ``the `return` clause takes 2 parameters, but this one has 1`` | ``the second parameter receives the state of the handler`` |
| 状態のない handler | 今のまま | 今のまま |

- `resume` の引数が2個でも3個でもなければ E1011 にする。メッセージは ``"`resume` takes a continuation, a value, and an optional state, but 1 argument was given"`` にする。

### 2.2 型検査

#### 状態の欄

継続の形 `TyShape::Cont` に状態の欄 `state: SlotId` を足す。欄は型の表とは別の union-find の変数で、値は次のどちらかである。

```rust
enum Slot {
    Stateless,
    State(Ty),
}
```

- 欄の単一化は、`Cont` どうしの単一化から呼ぶ。両方が決まっていれば、`Stateless` どうしは成功し、`State(a)` と `State(b)` は `a` と `b` を単一化し、種類が違えば `UnifyError::StateSlot` にする。決まっていない欄は、もう一方に束縛する。
- 子の型の走査 (`for_each_child`、occurs の検査、Kind の制約) は、`State(ty)` の `ty` を `Cont` の子として扱う。σ は `k` の Kind に関わらないので、`k` の Kind の制約は今のままである (effects.md)。
- 書き出す型は `Type::Cont { …, state: ContState }` とし、`ContState` は `Stateless`、`State(Box<Type>)`、`Unknown` のどれかにする。`Unknown` は誤りの後に解けなかった欄で、`Stateless` と同じく `from` を表示しない。`State(t)` は ``Cont Int Unit <IO> from File`` のように表示する。`table/export.rs` の `export` と `shape.rs` の `Closer::ty` は、status の注意どおり、この欄を手で扱う。

#### handle と `resume`

- handle: 初期値があれば先に推論し、その型を σ にする。節の `k` の型の欄は `State(σ)` に、状態のない handler では `Stateless` にする。節の最後の引数のパターンと、`return` の節の2つ目の引数のパターンは σ で検査する。
- `resume k v`: `k` を継続の型と単一化し、その欄を `Stateless` と単一化する。
- `resume k v s`: 新しい型の変数 σ' を作り、`k` の欄を `State(σ')` と単一化し、`s` を σ' で検査する。
- 欄の単一化が失敗しても、`v` と `s` の検査は続け、中の誤りを報告する。E2007 を出した `resume` の値の型は、継続の型の `ret` のままにする。

### 2.3 線形性と持ち越し

#### 使用回数のパス

- 初期値は、handle が1回使う。
- 節の状態の引数は普通の束縛なので、今の規則がそのまま当てはまる。`drop k` する節で状態を使わなければ E3003 になる。
- 合成した `_` は状態の型に `Unr` の制約を付ける。由来は `Origin::OmittedReturn { init: TextRange }` にする。この制約が破れたら、E3004 として報告する。

| 欄 | 内容 |
|---|---|
| メッセージ | ``the state of this handler is discarded by the omitted `return` clause`` |
| primary | `from` の初期値の式。ラベルは ``this state has a linear type `File` `` (型は実際の状態の型) |
| help | ``write a `return` clause that takes the state, such as `| return x st -> ...`, and consume the state there`` |
| fix | なし。`return` の節の本体を作れないためである |

#### 持ち越しのパス

- 初期値は本体より先に評価する。初期値を評価している間は、handle の後で使う値を持っている。今の handle の処理の前に、初期値の式をたどる。
- 状態は、handle の外側の row (`CallRows::Handle::outer`) をまたいで持つ値として、`carry_value` にかける。値の種類に `CarriedValue::HandlerState { init: TextRange }` を足す。
- E3006 の secondary は、状態のときは初期値の式を指し、ラベルを ``the state of this handler`` にする。

### 2.4 診断

#### `Fix`

```rust
pub struct Fix {
    /// 何をする fix か。LSP のコードアクションの題名になる。
    pub title: String,
    pub edits: Vec<TextEdit>,
}

pub fix: Option<Fix>,
pub fn with_fix(self, title: impl Into<String>, edits: Vec<TextEdit>) -> Self;
```

- E3003 の `drop x` の fix の題名は ``insert `drop x` `` にする (`x` は変数の名前)。
- 表示 (ariadne) で fix をどう見せるかは今のままにする。

#### E2007 `RESUME_STATE_MISMATCH`

primary は `resume` の式である。secondary は付けない。

| 場合 | メッセージ | ラベル | help |
|---|---|---|---|
| 状態のある `k` を2引数で再開した | ``this continuation comes from a handler with a state, so `resume` needs the next state`` | ``the next state is missing`` | ``pass the next state as the third argument: `resume k v st` `` |
| 状態のない `k` を3引数で再開した | ``this continuation comes from a handler without a state, so `resume` takes no state`` | ``the state argument is not expected`` | ``remove the third argument`` |

fix は次のとおり。

- 状態のない `k` への3引数: 題名 ``remove the state argument``。2つ目の引数の終わりから3つ目の引数の終わりまでを消す。常に付ける。
- 状態のある `k` への2引数: 題名 ``pass the current state `st` `` (`st` は変数の名前)。2つ目の引数の後に ` st` を入れる。次の3つをすべて満たすときだけ付ける。
  1. `k` の式が、`resume` を囲む操作の節の `k` の引数の変数をそのまま参照している
  2. その節の状態の引数のパターンが、変数の束縛 (型の明示を含んでもよい) である
  3. その変数の名前で `resume` の位置から引くと、同じ変数に解決する。E3003 の fix と同じく、後の束縛で隠されていないことを確かめる

状態が `Lin` で、節の中ですでに状態を使っていると、この fix を当てた後に E3002 になる。fix は状態をそのまま渡す提案であり、線形性までは見ない。

#### 番号の表の更新

diagnostics.md の「番号を割り当てていない診断」から E2xxx の行を除き、E2007 を表に足す。E3004 の行に「状態のある handler で省いた `return` の節が状態を捨てた場合は、`from` の初期値を指す」を足す。E3006 の行と「線形性の診断」の表の secondary に、handler の状態を足す。

### 2.5 Core IR とランタイム

6b-1 で通り道ができているので、変換が `init` に初期値のアトムを、`Resume::state` に3つ目の引数のアトムを入れるだけである。変換は、初期値のアトムを本体と節のクロージャより先に作る。ランタイムとインタプリタは変えない。

### 2.6 テストの変更と新しいテスト

#### 種類1 (事前の合意が要る)

| テスト | 変更 | 理由 |
|---|---|---|
| `eml_hir/tests/effects.rs` の `resume_and_drop_take_a_fixed_number_of_arguments` | 3行目の `resume k k k` を `resume k k k k` にし、期待値を E1011 の新しい文言にする。1行目の E1011 の文言も変わる | 3引数が正しい形になり、E1011 の文言が変わる |
| `eml_hir/tests/effects.rs` の `handlers_with_an_initial_state_come_in_stage_6` | 削除し、`from` の handler を変換した HIR のダンプのテストに置き換える | E0004 でなくなる |
| `eml_types/tests/check.rs` の `later_stage_constructs_add_no_type_errors` | `from` の handle の部分を除き、フィールドアクセスの E0004 だけを残す | E0004 でなくなる。残すと E1013 が出て、テストの意図からずれる |

#### 新しい UI テスト

| ファイル | 確かめること |
|---|---|
| `run/effects/state.em` | `run_state` で `get` と `put` が状態を読み書きし、`return` の節が最後の状態を受ける |
| `run/effects/state_unit.em` | `from ()` の handler を3引数の `resume` で再開する |
| `run/effects/state_multi.em` | `multi` の操作の `k` を2回、別の状態で再開し、再開ごとに状態が分かれる |
| `run/effects/state_omitted_return.em` | `return` の節を省いた状態のある handler が本体の値を返す (`debug_heap` で状態が解放される) |
| `run/files/state_file.em` | `File` を状態に持つ handler が、節で `read_all` して `resume` に渡し、`return` の節で `close` する |
| `run/files/state_released_on_abort.em` | 外側の `never` の操作による中断で、区間に入った handler フレームの状態の `File` が解放される |
| `check-fail/types/resume_state.em` | E2007 の2方向。fix が付く形と付かない形 (`k` を節の外の関数に渡した場合、状態の引数がパターンの場合) |
| `check-fail/linearity/state.em` | 省いた `return` の節の E3004、`drop k` した節で状態の `File` を消費しない E3003、外側の row の `multi` をまたぐ状態の E3006 |
| `check-fail/names/handler_state_arity.em` | 状態のある handler の E1010 (操作の節、`never` の節、`return` の節) と E1011 |

#### crate ごとのテスト

- `eml_hir`: `from` の変換 (初期値、節の状態の引数、合成した `return` の節)、`from` の handler の中の名前の誤り。
- `eml_types`: `Cont … from σ` の表示、`k` を節の外へ渡して3引数で再開する場合、`multi` の `k` を3引数で2回再開する場合、欄の不一致の E2007、`from ()` と状態のない handler の区別、E2007 の fix の3つの条件。
- `eml_core_ir`: `init` と `state` を持つ Core IR のスナップショット、verifier の節の引数の数の誤り。
- `eml_interp`: テキストの IR で、状態が `perform`、`resume`、`return` を通ること、写した区間の状態。

## 3. 文書の更新

| 文書 | 更新 |
|---|---|
| `docs/spec/diagnostics.md` | E2007、E3004、E3006、「線形性の診断」の表、`Diagnostic` の `fix` の形 (2.4) |
| `docs/spec/expressions.md` | 省いた `return` の節を `Lin` の状態で書いたときの E3004 への参照 |
| `docs/spec/linearity.md` | 状態の持ち越しでまたぐ row が外側の row であること (D5) |
| `docs/spec/core-ir.md` | `Atom::Fn`、`Perform::resumable`、テキストの形、verifier の節の引数の数 (1.2) |
| `docs/spec/runtime.md` | handler フレームの `Link` と、状態をフレームの子として数えること (1.3) |
| `docs/implementation/status.md` | 段階6b を完了にし、「段階6b の spec への入力」と 6b の注意点を消す。`crate` の表を更新する |
| `docs/implementation/test-changes.md` | 種類1と種類2の変更を記録する |

## 4. 範囲に入れないもの

- evidence passing の最適化 ([evidence passing の設計](../../future/evidence-passing.md))
- 線形なクロージャを `data` に入れること、線形な値を多相な操作の引数で渡すこと (status の注意点のまま)
- E2007 の secondary に、`k` を作った handle を指すラベルを付けること。継続の型は由来を持たないので、付けるには型の表に欄の由来を足す必要がある
- 網羅されていない `match` の fix など、仮置きの式を前提にする fix
