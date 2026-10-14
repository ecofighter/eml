# S9b-2b 実行文脈 `Ctx` と基本操作

位置づけ: S9b-2b の設計。終えたら決めたことを `docs/spec/object-model.md` と `docs/spec/lir.md` に移し、この文書を消す。

## 目的と範囲

[ロードマップ](../../future/roadmap.md) の S9b-2b (「`Ctx`、基本操作、SIGINT」) を作る。

- `eml_rt` に実行文脈 `Ctx<M>` を置く
- [LIR とインタプリタ](../../spec/lir.md) の「基本操作」の表のうち、すでにある `dummy` を除く19個を `Ctx` のメソッドとして作り、動作まで Verus で証明する
- 安全点の確かめ (割り込み、深さの上限、確保量の上限) を作る
- SIGINT の受け方を決める。`eml_rt` の側 (止める語) を作り、ハンドラは S10 で `eml_cli` が入れる

判断の基準は、簡潔で洗練された実装を第一に、次に性能である。S10 の VM、ネイティブ化、VM の並列化で要ることも含めて決める。

設計の案は、2つの観点で敵対的なレビューにかけた。安全性と検証のしやすさ、VM とネイティブのコードからの使いやすさと性能である。この文書は、その指摘を反映した形である。

## `Ctx` の形

```rust
pub struct Ctx<M: Mode> {
    heap: Heap<M>,
    evv: Word,                 // 今の evv。所有した参照。空は即値の ()
    yielding: YieldState,      // #[repr(C)] { target, clause, cont: Word, abort: bool }。target が () ならなし
    control: Arc<Control>,     // セッションに1つ (下の「実行の制御」)
    max_depth: usize,          // control の値の写し
    max_evv: usize,            // evv の長さの上限。control の値の写し
    stats: CtxStats,           // evidence_steps、yields、yield_frames。検査モードだけで数える
}
```

- 基本操作を `Ctx` のメソッドにする。VM は `&mut Ctx` だけを持ち回す。`Heap` と実行の状態を別の構造体にする案は採らない。持ち回すものが2つになり、基本操作の契約が2つの値をまたぐためである
- `Ctx` は `heap/ctx.rs` に置き、`lib.rs` から公開する。ヒープの spec の関数はどれも `pub closed` で、`heap` の子のモジュールからしか中を開けないためである (`string.rs` と同じ置き方)。`trust.rs` の `FILES` に足す
- `Ctx` は `Send` で `Sync` でない (`Heap` と同じ)。型の assert で固定する
- 論理的な worker ID は、今と同じく `Heap` に置く。確保のたびに物体の所有者を書くのは `Heap` だからである。ロードマップの「`Ctx` に worker ID を置く」の文を直す
- ロードマップが `Ctx` に挙げる欄のうち、出力先は extern と一緒に S9b-3 で、セッションの根は S11 で足す。使う段より前に足すと、テストできない欄になるためである
- `heap()` は安全なメソッドにする。`heap_mut()` は `unsafe fn` にする。安全なコードがヒープを入れ替えると、`Ctx` の持つ語が、落としたヒープを指すことになるためである。前提は「ヒープを入れ替えない。`Ctx` の持つ語を落としたり書き換えたりしない」である
- `Ctx` を落とすとき、`evv` と yield の状態の参照は個別に手放さない。ヒープと一緒に消える。`Ctx` の `Drop` はこれらの語に触れないので、基本操作の途中の `decref` で破棄処理が panic しても、落とすことは安全である (信頼の一覧の T7 に書く)
- `check_finished()` は、`evv` が空で yield の状態がなしであることを確かめる。VM がリークの検査の前に呼ぶ ([LIR とインタプリタ](../../spec/lir.md) の「`evv` の不変条件」の最後の項)

### yield の状態

- 平らな `#[repr(C)]` の記録 `{ target, clause, cont, abort }` にし、`target` が `()` のときを「なし」とする。Rust の enum は配置が決まらないが、この形なら、ネイティブのコードが「yield 中か」を決まった位置の語と即値の1回の比較で読める (Koka の `yielding` の欄と同じ形)
- VM から見た動作は enum と変わらない。不変条件は「`target` が `()` なら、ほかの3つも `()` か `false`」の1行である

### タスクの切り替え

[LIR とインタプリタ](../../spec/lir.md) の「実行の文脈」は、S13 の協調的な `par` で、実行の文脈をタスクごとに持つか、切り替えで退避するかを求める。次のように決める。

- タスクは、yield の状態がなしの安全点でだけ切り替える。そのため、退避するのは `evv` だけで、`evv_get` と `evv_set` で足りる。基本操作を足さない
- `par` の子は `evv = ()` で始める。子の行は `<>` なので、evidence がタスクをまたがない。これは [オブジェクトモデル](../../spec/object-model.md) の「evidence の3つの物体」が、状態の欄をその場で書き換える条件として挙げたことでもある

## 実行の制御

`Control` は、セッションに1つの、実行を止める情報である。`Arc<Control>` で、CLI と、どの worker の `Ctx` とヒープも同じものを見る。

```rust
pub struct Control {
    stop: AtomicU32,        // 止める理由のビット: INTERRUPT、MEMORY
    bytes: AtomicUsize,     // セッションのヒープが OS から取ったページのバイト数
    max_bytes: usize,       // 確保量の上限。既定は usize::MAX
    max_depth: usize,       // 深さの上限 (VM のスタックの語の数)。既定は usize::MAX
    max_evv: usize,         // evv の長さの上限。既定は usize::MAX
}
```

- 安全点の確かめは `Ctx::poll(&self, depth: usize) -> Result<(), Stop>` である。速い道は `depth > self.max_depth || self.control.stop.load(Relaxed) != 0` の比較2つで、どちらも偽なら `Ok` を返す。真なら遅い道で `Stop` (`Interrupted`、`DepthLimit`、`MemoryLimit`) を決める。VM が呼び出しと戻りで呼ぶ (S10)
- 深さは、VM のタスクが持つスタックの語の数で数える。フレームの数で数えないのは、スタックの大きさを抑えるのが目的だからである
- 確保量は、確保器がページを取るときと返すときに、`Control::bytes` を増減して数える。上限を超えたら `stop` の `MEMORY` を立てる。ページの単位 (64 KiB) でしか数えないので、速い道の費用はない。worker ごとのヒープの数を足し合わせた、セッション全体の量になる。凍結した塊のページも、塊が生きている間は数に入る (ページを `Frozen` に移しても、数はそのまま残り、塊を落とすときに減る)
- 確保器に `Control` を渡すため、`Heap` は `Arc<Control>` を持つ。`Heap::new()` は上限のない新しい `Control` で作り、`Heap::with_control(control)` で共有のものを渡す。`Ctx::new(control)` は後者を使う
- `Control::interrupt()` は `INTERRUPT` を立て、`Control::clear()` はビットを下ろす (S11 の REPL が入力の始めに呼ぶ)。上限の値は `Control` を作るときに決め、変えない
- 上限の既定の値と `RunConfig` での指定は、VM と一緒に S10 で決める
- `poll`、`interrupt`、`clear` は `verus!` の外の安全なコードで書く。`leaks` や `check_invariants` と同じ置き方で、何も約束しないので信頼の一覧の行は足さない。ページの数の増減は、T1 (ページの確保と返却) の行に足す。値について何も約束しない atomic の加減である

## SIGINT

- `eml_rt` は止める語だけを持ち、vstd だけに依存したままにする
- OS のハンドラは `eml_cli` が ctrlc crate で入れ、`Control::interrupt()` を呼ぶ。`eml run` では、`INTERRUPT` がすでに立っていれば (2回目の Ctrl-C)、その場で終了コード 130 で終わる。S11 の REPL は2回目でも終わらないので、2回目の扱いはハンドラの設定にする
- 今の CEK は止める語を見ないので、ハンドラを入れるのは VM に切り替える S10 にする。今入れると、Ctrl-C で止まらなくなるためである
- ネイティブでは、生成したプログラムの始めで同じ形で入れる
- 並列 VM では、どの worker の `Ctx` も同じ `Control` を見る

## 基本操作

### spec の関数

ghost の状態は `Ctx` に足さず、ヒープの上の spec の関数で表す。VM は検証しないコードで、基本操作の合間にヒープを変えるので、`Ctx` の中で ghost のリストを保ち続けられないためである。

- `ev_shape(e)`: `e` は読める `Data` の物体で、走査する語が3つ (入口の表、状態、外側の `evv`) で、tag が 0 でない。`lin` ビットがなく、`owned` である。入口の表は読める tag 0 の `Data` の物体で、`lin` ビットがなく、どの欄も `()` か、`lin` ビットのない `owned` の語である。再帰しない浅い形である
- `evv_path(w, fuel) -> Option<Seq<(u16, Word)>>`: `w` から節点をたどり、`fuel` 歩以内に `()` に着くなら、(tag、evidence) の組の列を返す。どの節点も読める `Data` で、走査する語が2つ ([evidence, next]) で、`lin` ビットがなく、tag はその evidence の tag と同じで、evidence は `ev_shape` を満たす。着かない (輪になる、形が違う) なら `None` である。`decreases fuel` で定める
- `evv_seq(w)`: ある `fuel` で `evv_path` が `Some` ならその列。`evv_wf(w)` は、そうなる `fuel` があり、列の長さが `max_evv` 以下であることである
- `mask_spec(effects, seq)`: `effects` の多重集合の各エフェクトについて、先頭から tag の合う組をその数だけ除いた列。組が tag を持つので、ヒープを読まずに定まる
- `held_ok()`: `Ctx` の持つ語の前提。`evv_wf(self.evv)` で、`self.evv` と yield の状態の3つの語をこのヒープが持っている (`holds`)。検査モードでは、4つの語が `word_in` である

外側の `evv` が `evv_wf` であることは、`ev_shape` に入れない。入れると spec が互いに再帰し、検査モードで確かめる費用が、handler の入れ子の深さ d に対して 2^d になるためである。外側の `evv` が正しいことは EP の不変条件として速いモードの前提にし、`ev_evv` の結果は語そのものだけを約束する。

### evidence と節点の tag

evidence と `evv` の節点の tag は、エフェクトの番号に 1 を足したものにする。入口の表とクロージャの tag は 0 なので、検査モードが evidence とクロージャを見分けられる。`ev_lookup[E]` は E + 1 の `meta` を1回作り、節点ごとに `meta` 全体と比べる。エフェクトの数の上限は 65535 になる。

### 契約

- どの基本操作も `held_ok()` を前提にし、保つ。VM の義務は「基本操作の合間に、`Ctx` の持つ語に触れない」になる
- ヒープの語を読む基本操作は、核の操作と同じく `unsafe fn` にする。ヒープに触れない `yielding`、`yield_status`、`yield_cont_take`、`check_finished` は安全な関数にする。前提を崩しても、リークするだけである
- どの操作も、RC のモデルでの増減を ensures に書く。借用の引数 (表の「借用」) は手放さず、ほかは消費する ([LIR とインタプリタ](../../spec/lir.md) の「基本操作」)。消費する引数は `holds` を、`dup` する語は `lin` ビットがなく `owned` であることを、書く物体は `writable` を前提にする

| 操作 | 前提 (速いモード) | 結果と動作 |
|---|---|---|
| `evv_get()` | | `self.evv` を `dup` して返す |
| `evv_set(l)` | `evv_wf(l)`、`holds(l)` | `evv` を l にし、元の `evv` を `decref` する |
| `evv_push(l, e)` | `evv_wf(l)`、長さが `max_evv` 未満、`ev_shape(e)` | 新しい節点 (tag は e の tag)。`evv_seq(結果) == [(tag, e)] + evv_seq(l)` |
| `evv_mask[effects](w)` | `evv_wf(w)`、合う組が足りる | `evv_seq(結果) == mask_spec(effects, evv_seq(w))`。除く位置までの節点を写し、残りを共有する |
| `ev_new[E](state, outer, n_ops)` | `evv_wf(outer)`、E < 65535 | 欄が `n_ops` 個で、どれも `()` の入口の表と、それを指す tag E + 1 の evidence |
| `ev_set_clause[op](ev, c)` | `ev_shape(ev)`、op が範囲の中、欄が `()`、表が `writable` | 入口の欄 op を c にする |
| `ev_renew(ev, state)` | `ev_shape(ev)` | 同じ入口の表を指し、状態を引数に、外側を今の `evv` にした新しい evidence |
| `ev_lookup[E]()` | tag E + 1 の組がある | `evv_seq(self.evv)` で tag が E + 1 の最初の evidence を `dup` して返す |
| `ev_clause[op](ev)` | `ev_shape(ev)`、op が範囲の中 | 入口の欄 op を `dup` して返す |
| `ev_evv(ev)` | `ev_shape(ev)` | 外側の `evv` (`scanned_word(ev, 2)`) を `dup` して返す |
| `ev_take_state(ev)` | `ev_shape(ev)`、`writable(ev)` | 状態を返し、欄を `()` にする |
| `ev_set_state(ev, s)` | `ev_shape(ev)`、`writable(ev)`、ev をこのヒープが持っている | 状態の欄を s にし、元の状態を `decref` する |
| `yielding()` | | yield 中なら 1、そうでなければ 0 |
| `yield_status(ev)` | | 0 / 1 / 2 / 3 ([LIR とインタプリタ](../../spec/lir.md) の表)。宛先は語の値で比べる |
| `yield_to(ev, clause, k)` / `yield_abort(…)` | yield の状態がなし | yield 中 (中断中) にする。3つの参照を持つ |
| `yield_cont_take()` | yield 中 | κ を返し、欄を `()` にする |
| `yield_cont_set(k)` | yield 中で κ が `()` | κ を入れる |
| `yield_finish()` | yield 中で κ が `()` | 宛先への参照を手放し、節の本体を返し、状態をなしにする |

- `ev_set_state` は、ev を持っていること (`ev` の外の参照が1つ以上) を前提にする。元の状態を手放す連鎖が ev 自身を空けないようにするためである ([LIR とインタプリタ](../../spec/lir.md) の「evidence」の、状態の欄を通る輪)
- `ev_new` は入口の数を引数で受ける。これは S10 までのつなぎでなく、そのままの形である。VM とネイティブのコードは、エフェクトの操作の数をコードの定数として持つためである
- `yield_status` の宛先の比較は、語の値 (番地) で行う。yield の状態が宛先への参照を持つので、比べている間に番地が使い回されることはない
- evidence (3語) と節点 (2語) は、`field` と `set_field` で読み書きする。速いモードで1回の読み込みで済むためである。入口の表は、欄が 255 個以上なら先頭に数の語があるので、`scanned` で読み、書くときは位置をずらす
- `evv_mask` の即値は `&[u16]` (エフェクトの番号の多重集合) である。写す節点は先頭から順に作り、1つ前の写しの `next` を `set_field` で書く。Rust の `Vec` は使わない。多重集合は、節点ごとに即値を見直して数える (エフェクトの数 m は小さいので、費用は O(n·m))
- evidence と節点は不死にならない (`freeze` はヒープを消費し、evidence は定数にしない)。後の `par` で継続のクロージャが共有されれば、evidence は共有になりうる。そのとき `ev_set_state` は検査モードで `set_field` の `NotWritable` を返し、速いモードでは前提を崩す。そのため `writable(ev)` を前提に書いておく

### ヒープの側に足すこと

基本操作の証明は、ヒープの核の操作の組み合わせである。次の約束を核の操作に足す。中で証明済みの事実を外に出すだけで、実行するコードは変えない。

- `decref`、`release`、`release_reuse` の ensures に「呼ぶ前と後の両方で生きているブロックは、`meta`、走査する語、文字列のバイトを変えない」を足す。これがないと、途中の `decref` の後に、外の参照を持たないリストの後ろの節点や evidence について何も言えず、`evv_wf` を保てない
- 検査モードの `&mut self` の操作 (`alloc*`、`dup`、`decref`、`release*`、`set_field`、`set_raw`、`acquire_immortal`、`attach`、`set_worker`) の ensures に、「前に `word_in` だった語は、後も `word_in` である」を足す。検査モードはブロックを使い回さないので成り立つ
- 読める物体の走査する語は読める、という補題を公開する
- `alloc_data` の ensures に、tag、`lin` ビット、走査する語がすべて `()` であることを足す

### 速いモードと検査モード

- 核の操作と同じ分け方にする。速いモードは上の表の前提を安全の前提にし、検査モードは確かめて `Err` を返す
- 検査モードは、各操作が読む所だけを確かめる。`ev_lookup` と `evv_mask` は、たどる節点ごとに形と tag を確かめ、`ev_*` は evidence の形を確かめる。`evv_set` と `evv_push` は、リスト全体をたどらない
- リストをたどるループは、`max_evv` 歩を超えたら `Err` にする。`evv_wf` が長さを `max_evv` 以下と定めるので、超えるのは定義から前提を崩したときだけである。Verus の停止の証明にもこの数を使う。既定の `max_evv` は `usize::MAX` で、輪になったリストのテストは小さい上限で作る
- 状態を変える前に、検査モードで確かめることをすべて確かめる。たとえば `evv_push` は、確保の前に e の形を確かめる。`Err` のときヒープは変わらない。ただし `evv_set`、`ev_set_state`、`yield_finish` は、手放す連鎖の途中で誤りを見つけうる (`chain_err`)。[オブジェクトモデル](../../spec/object-model.md) の「検査モードの約束」の、状態を変えた後に `Err` を返しうる操作の一覧に足す。その後は `Ctx` を落とすことしかできない
- 新しい誤りは次の3つである。`HeapError` に足す。ヒープの誤りと1つの型にしておくと、VM が扱う誤りの型が1つで済むためである
  - `BadEvidence`: 節点、evidence、入口の表の形が違う。入口の欄に2回入れた
  - `NoHandler`: 合う evidence がない (`ev_lookup`、`evv_mask`)
  - `YieldMisuse`: yield の状態の前提が崩れている

### 性能

- `perform` の道は `ev_lookup` と `ev_clause` である。速いモードの `ev_lookup` は、節点ごとに `meta` を1回読んで定数と比べ、次の節点を1回読むだけにする。`evv_mask` は `mask` 付きの呼び出しだけが通り、`perform` の道にない
- 仕事の回数 (`evidence_steps`、`yields`、`yield_frames`) は、`Heap` の `Stats` と同じく検査モードだけで数える。`bench/` のスナップショットは `debug_heap` 付きで測るので、この分け方と合う。今のインタプリタの数え方のテスト3つ (`eml_interp/src/prims.rs` の tests) を、同じ期待値で `eml_rt` に移す。S10 の VM は、その定義を引き継ぐ
- `bench/rt.sh` に次の計測を足し、S9b-2b の基準にする
  - `perform`: handler の中で `perform` してその場で戻る形
  - `perform-deep`: 8つの handler の奥にある evidence への `perform`
  - `handle`: handler を設けて外す1回 (`ev_new`、`ev_set_clause`、`evv_push`、`evv_set`)
  - `yield`: yield して継続を組み立て、再開する1往復
  - `mask`: `evv_mask` の1回
  - `poll`: 安全点の確かめの1回

## 信頼の一覧

新しい T の番号は作らない。

- T1 (ページの確保と返却): セッションのバイト数の atomic の加減を足す。値について何も約束しない
- T7 (unwind): 基本操作の中の `decref` で破棄処理が panic しても、`Ctx` の `Drop` は `evv` と yield の状態の語に触れないので、落とすことは安全である
- `poll`、`interrupt`、`clear` は `verus!` の外の安全なコードなので、行を足さない

## 検証とテスト

- Verus: crate 全体が 0 errors で通る
- 単体テスト (`both_modes!`)
  - 19個の基本操作のそれぞれ
  - EP が出す形を並べた筋書き。handle の中で `perform` してその場で戻る、`mask` が合う evidence を飛ばす、yield して継続を組み立てて再開する1往復。どれも終わりに `check_finished` が通る
  - 検査モードの3つの誤りと、状態の欄への `NotWritable`
  - 輪になった `evv` が、小さい `max_evv` で `BadEvidence` になって止まること
  - 入口の欄が 255 個以上の表への `ev_set_clause` と `ev_clause`
  - インタプリタから移す、数え方のテスト3つ
  - `poll`: 割り込み、深さの上限、確保量の上限のそれぞれで `Stop` を返すこと。`Control::clear` で戻ること。2つのヒープのページが1つの `Control` に足し合わされること
  - 型の assert: `Ctx` は `Send` で `Sync` でない。`Control` は `Send + Sync` である
- Miri (`trusted::`)
  - 別のスレッドが `Control::interrupt()` を呼び、`poll` が `Stop::Interrupted` を返すこと
  - 2つのスレッドのヒープが、1つの `Control` にページの数を足し引きすること (T1)
  - 核の操作を1回ずつ通す T8 のテストに、基本操作を足す
- 計測: 今ある計測は S9b-2a から +0.5% を超えて増えない。`Heap` に欄を足すと速い道の関数がインライン化されなくなることがあった (「S9b-2a (凍結した領域)」の記録) ので、`Arc<Control>` の欄で同じことが起きないかを確かめる

## 文書

段の終わりに1回で直す。

- `object-model.md`
  - 新しい節「実行文脈 `Ctx`」: 欄、yield の状態の形、タスクの切り替え、`Control` と `poll`、上限、止める語、`check_finished`
  - 「evidence の3つの物体」: tag をエフェクトの番号 + 1 にすること、エフェクトの数の上限 65535
  - 「論理的な worker ID」の「まだ決めていないこと」を閉じる
  - 「報告する誤り」に3つの誤り。「検査モードの約束」に、状態を変えた後に `Err` を返しうる基本操作
  - 「核の操作」に、`decref` と `release` の生きているブロックの約束と、`alloc_data` の約束を足す
  - 信頼の一覧の T1 と T7
- `lir.md`: 「実行の文脈」と「基本操作」に、`eml_rt` の `Ctx` が実装すること、yield の状態の形、タスクの切り替え、`ev_new` が入口の数を引数で受けること
- `roadmap.md`
  - S9b-2b を終えた状態にし、論点「SIGINT の受け方」を閉じる
  - S9 の決めたことの `Ctx` の欄の一覧を直す (worker ID は `Heap`、出力先は S9b-3、セッションの根は S11)
  - S10 の決めたことに、ハンドラ (ctrlc、`eml run` では2回目で終了コード 130)、上限の既定の値と `RunConfig`、`RunStats` の仕事の回数は `eml_rt` の検査モードの定義を引き継ぐこと
  - S11 に、入力の始めの `Control::clear` と、2回目の Ctrl-C で終わらないこと
- `multicore.md`: `Control` はセッションに1つで、worker が共有する。タスクは yield の状態がなしの安全点でだけ切り替え、`par` の子は `evv = ()` で始める
- `testing.md`、`benchmarks.md`、`docs/README.md`、`CLAUDE.md` の `eml_rt` の段落

## テストの変更

`eml_rt` はまだパイプラインから使わないので、UI テストの出力も `bench/` のスナップショットも変わらない。成否の変更と期待値の変更はない。インタプリタの数え方のテストは、写して `eml_rt` に足すだけで、元のテストは変えない。

## 完了の条件

次がすべて通ることを完了の条件とする。

- `cargo test`
- `cargo clippy --all-targets`
- `cargo fmt`
- `verusfmt --verus-only --check`
- `cargo verus verify -p eml_rt` (0 errors)
- `trusted::` の Miri (2つの設定)

さらに、`bench/rt.sh` の今ある計測が S9b-2a から +0.5% を超えて増えないこと。
