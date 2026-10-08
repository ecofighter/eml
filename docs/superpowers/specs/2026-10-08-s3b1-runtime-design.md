# S3b-1 ランタイムとインタプリタの土台 (設計)

位置づけ: 作業の設計。再設計のサブプロジェクト S3b の前半、S3b-1 の spec である。全体の方針は [ロードマップ](../../future/roadmap.md) の「S3b バックエンドの土台」と、[全体設計](2026-10-07-redesign-design.md) にある。決まったことは S3b-1 の終わりに `docs/` の該当文書へ移し、この文書を削除する。

## 目的

ランタイムとインタプリタを、Core IR v2 (S3b-2) とネイティブ化の前に整える。形だけのマルチコアの備えを削除し、文字列のリテラルを不死の物体にし、一意な文字列の連結をその場で行い、`perform` が handler の連鎖だけをたどるようにする。Core IR のデータ型とテキストの形は変えない。言語の意味と UI テストの出力も変えない。

## S3b の分け方

ロードマップの S3b を2つの段に分ける。

- S3b-1 (この文書): ランタイムとインタプリタ。Core IR を変えずに進められる項目である
- S3b-2: Core IR v2。ブロック木、テキストの形、verifier と liveness と Perceus の作り直し、pattern.rs の作り直し (タプルを決定木の複数の列にすること、`&&` と `||` を条件の分岐にすること)、translate で作る末尾呼び出し、縮約パス、scrutinee を消費しない `Switch`、Repr、extern の呼び出しの位置である

分ける理由は次のとおりである。

- ランタイムの項目は Core IR に依存しないので、先に入れられる。エフェクトを使う再帰と文字列の連結が2乗にならないことは、S3b-1 だけで確かめられる
- Core IR の項目は、テキストの形の移行 (インラインスナップショットが約73件、手書きの IR が約80本) を1回で済ませるために、1つの段にまとめる。Perceus と verifier の作り直しも1回で済む

## 背景

- ランタイムのマルチコアへの備えは形だけである。`Header.rc` の `AtomicI32` は `get_mut` からしか触られない。`mark_shared` は `NotImplemented` を返し、テストのほかに呼び出し元がない。記述子は名前しか持たず、読むのは `live_objects` だけである。`OutputSink` の `Arc<Mutex>`、`FileHandle.reader` の `Send + Sync`、`Arc<Program>` の理由は「将来のスレッド」だけで、`Arc` を複製する箇所はない
- `runtime.md` は「記述子は `Lin` の破棄処理を持つ」と書くが、実装はない。インタプリタでは `Payload` の enum がレイアウトで、Rust の `Drop` が破棄処理である。`children` が `Data`、フレーム、クロージャの中の `File` までたどるので、捨てた `k` が捕まえた `Lin` の値も今のインタプリタで後始末される。ロードマップの S3b の論点「ネイティブで包んだ `k` の破棄処理」は、ネイティブのオブジェクトモデルの要件である
- `Rhs::ConstString` は、実行のたびに文字列を写して新しい物体を作る
- `take_string` は、文字列が一意でも必ず写す。`++` は両辺を写して3つめの物体を作るので、ループで文字列を伸ばすと2乗の時間がかかる
- `find_handler` は、継続のすべてのフレーム (`Return`、`Apply`、`Mask`、handler) をたどる。handler の下の非末尾の再帰で `perform` すると、1回ごとに深さに比例する時間がかかる
- 今のテストは、2乗にならないことを確かめられない。`tests/ui/run/runtime/` のテストは終わることだけを確かめ、フレームはヒープにあるので、フレームを積む実装でも通る
- `Mask` のフレームの数は handler のフレームの数を超えない ([エフェクトと handler](../../spec/effects.md) の「健全性」)。handler と `Mask` のフレームだけをたどれば、`perform` の費用は handler の数に比例する

## 決めたこと

### シングルスレッドのランタイム

- `Header.rc` を `u32` にする。「正なら局所、負なら共有」のコメントは削除する
- `Heap::mark_shared` と `HeapError::NotImplemented` を削除する
- `DescId`、`Descriptor`、`DESCRIPTORS`、`Header.desc`、`Payload::desc` を削除する。`live_objects` は、新しい `Payload::kind_name() -> &'static str` で種類ごとに数える。名前は今の記述子と同じ (`String`、`Frame`、`Closure`、`Continuation`、`Data`、`File`) なので、リークの報告は変わらない
- `FileHandle.reader` を `Box<dyn Read>` にする
- `OutputSink` を `Rc<RefCell<dyn Write>>` にし、`Captured` を `Rc<RefCell<Vec<u8>>>` にする。`OutputSink` をスレッドの間で受け渡す箇所はない
- `Arc<Program>` をやめる。`Compiled.program` は `Option<Program>` にする。`eml_cli::execute` と `eml_interp::run` は `&Program` を受け取る。`eml_test_support` の `core*` は `Program` を返す
- `RunConfig` の doc comment から `threads` の話を削除する。`#[non_exhaustive]` は、フィールドを足しても呼び出し側を壊さないために残す
- `eml_cli::Session` は `Send + Sync` のままにし、`eml_cli/tests/api.rs` の assert も残す。理由はフロントエンドの側 (salsa への載せ替えと並列のコンパイル) にあり、インタプリタとは関係ない
- 世代番号は `u32` のままにする。その場の連結はスロットを解放して取り直さないので、同じスロットの世代が速く進むことはない
- 値とフレームに `Rc` と `RefCell` を使わない規則 ([Core IR とインタプリタ](../../spec/core-ir.md)) は残す。理由は、`dup` と `decref` の命令が自前のヒープの数を直接増減することにある。`OutputSink` は値でもフレームでもない

### 不死の文字列リテラル

意味 (`spec/runtime.md` に書く)。

- 不死の物体は解放されない。一意にならないので、`is_unique` は偽を返し、`take` は `Shared` で断り、`take_or_copy` は写す
- RC の操作を飛ばしてよいのは、検査付きヒープを持たないバックエンド (ネイティブのランタイム) だけである。Core IR は、リテラルについても `dup` と `decref` の釣り合いを保つ。値がリテラルだからといって `dup` や `decref` を消すパスは作らない
- 文字列のリテラルは不死の物体である。`Machine::new` が、`Program::strings` の項目ごとに不死の物体を1つ作る。`Rhs::ConstString(i)` は、その物体を返す

検査付きヒープ (インタプリタ) での扱い。

- `Header` は `rc: u32` と `immortal: bool` を持つ。不死の物体の `rc` は、外に出ている参照の数である
- `ConstString` は、専用の `Heap::acquire_immortal` で数を1増やす。`dup` は1増やし、`decref` は1減らす。数が 0 になっても解放しない
- 数が 0 の不死の物体には、どの操作 (`get`、`get_mut`、`dup`、`decref`、`is_unique`、`take`、`take_or_copy`) も `UseAfterFree` を返す。判定は `object()` と `object_mut()` の1か所に置く。`acquire_immortal` だけがこの判定を通らない
- `debug_heap` の実行が正常に終わったとき、数が 0 でない不死の物体は、その数だけリークとして種類の名前の下に足す。数が 0 の不死の物体は報告しない。今のリークを報告するテスト (`const "leaked"` を手放さない) は、今と同じく `[("String", 1)]` を報告する
- 弱くなる検出が1つある。同じリテラルを何回評価しても、参照は1つの数にまとめられる。片方の参照を余分に `decref` し、もう片方を手放し忘れると、2つの誤りが打ち消し合って見つからない。今はリテラルを評価するたびに別の物体を作るので、この形も見つかる。この限界は `runtime.md` の `debug_heap` の節に書く

### 文字列を余計に写さない

- `eml_runtime` に `Heap::append_str(left, right) -> Result<usize, HeapError>` を足す。左辺の `String` の後に右辺の文字列を足し、写したバイト数を返す。左辺の `String` をいったん取り出し、右辺を借りて足してから戻す
- `++` (`Extern::StrConcat`) は、左辺が一意なら `append_str` で足し、右辺を `decref` して左辺を返す。左辺が一意でない場合 (共有された文字列と不死のリテラル) は、両辺の長さの和の容量で新しい文字列を作り、両辺を `decref` する。`x ++ x` は Perceus の `dup` で RC が 2 になって届くので、写す側に進む
- `take_string` (写してから `decref`) をやめ、借りて使ってから `decref` する形にする。`println`、`StrEq`、`StrNe` は写さなくなる。`open` は、`FileHandle` と誤りの文言のためにパスを1回だけ写す

### handler の連鎖

`perform` は、handler と `Mask` のフレームだけをたどる。

表現は次のとおりである。

- `Machine` は `handlers: ObjRef` を持つ。`cont` から `next` でたどって最初に会う handler か `Mask` のフレームを指し、どちらもなければ `Root` のフレームを指す
- `Frame::Mask` は、所有しない参照 `outer` を持つ。外側の次の連鎖のフレームを指す
- handler のフレームの `link: Option<Link>` を、enum `Attachment` に変える。`Attached { next, state, outer }` は、つながっている間の外側 (所有する `next`)、状態、外側の次の連鎖のフレームである。`Detached { inner }` は、継続に捕まえられて切り離されている間の形で、`inner` は捕まえた区間の中でいちばん内側の連鎖のフレームを指す。区間に連鎖のフレームがなければ、その handler 自身を指す
- `inner` を切り離した handler のフレームに置くので、`Payload::Continuation { top, handler }` は変わらない
- `outer` と `inner` は所有しない。`children` はこの2つをたどらず、RC は変わらない

連鎖を書き換える箇所は次のとおりである。

| 動作 | 連鎖の書き換え |
|---|---|
| `Mask` を積む (`mask` 付きの呼び出し、末尾呼び出し、`resume`) | `outer = handlers` にし、`handlers` をそのフレームにする |
| handler を積む (`Call::Handle`) | `Attached.outer = handlers` にし、`handlers` をそのフレームにする |
| `ret` が `Mask` を外す | `handlers = outer` |
| `ret` が handler を外す | `handlers = Attached.outer` |
| `perform` | `handlers` から `outer` をたどり、今と同じ飛ばす数の規則で handler h を見つける。h の `Attached { next, state, outer }` を読み、`cont = next`、`handlers = outer` にする。h を `Detached { inner: 元の handlers }` にする。`never` の操作は区間を解放する |
| `resume` | h の `Detached { inner }` を読む。h を `Attached { next: cont, state, outer: handlers }` にし、`cont = top`、`handlers = inner` にする。`mask` 付きの `resume` では、その前に積んだ `Mask` が `handlers` になっている |
| `copy_segment` | 区間のフレームを写すとき、区間の中の `outer` と h の `inner` を、元のフレームから写したフレームへの対応で付け替える。対応にない参照は `HeapError::BrokenSegment` にする |

不変条件は次の2つである。

- `handlers` は、`cont` から `next` でたどって最初に会う連鎖のフレーム (handler、`Mask`、`Root` のどれか) である
- 捕まえた区間の中の連鎖の参照は、区間の外を指さない。区間のいちばん外側の連鎖の参照は h を指す

検査はすべて O(1) で、`debug_heap` によらず常に行う。今のインタプリタも、内部の不変条件は `Fault::Internal` で常に検査している。

- `ret` が `Mask` か handler を外すとき、そのフレームが `handlers` と同じであることを確かめる
- `perform` のたどりが、`Attached` の handler で終わることを確かめる
- `resume` で、h が `Detached` であり、`inner` が handler か `Mask` のフレームであることを確かめる
- `copy_segment` は、区間をすべてたどる。そのついでに、区間の中の連鎖が `inner` から `outer` で h までつながっていることを確かめる

`perform` ごとに継続の全体をたどる検査は入れない。`debug_heap` は UI テストとテストの補助で常に付くので、全体をたどるとどのテストも2乗の時間になる。`debug_heap` の意味 (リークと解放済みの参照の検出) は変えない。

### 仕事の回数

`eml_interp` は、実行の仕事を数える `RunStats` を持つ。

- `handler_visits`: `find_handler` が調べたフレームの数。今は継続のすべてのフレームを数え、連鎖を入れた後は連鎖のフレームだけを数える
- `string_bytes_copied`: インタプリタが文字列の物体の中身に書いたバイト数。今は `ConstString` と `take_string` が写す分を数える。連鎖と不死のリテラルを入れた後は、`++` の写す側の |左辺| + |右辺| と、その場で足す側の |右辺| を数える。`String` の容量の再確保と、出力への書き込みは数えない

API は次のとおりである。

- `eml_interp::run` は `Result<RunStats, RuntimeError>` を返す。実行時エラーとリークのときは `RunStats` を返さない
- `eml_cli::execute` は `RunStats` をそのまま返す。CLI は使わない
- `eml_test_support` の `run_program` は `RunStats` を受け取る。`run`、`run_files`、`execute` は今の戻り値の型のままにし、`RunStats` を捨てる。新しく `run_stats(text) -> (String, Result<RunStats, RuntimeError>)` を足す。`run_stats` も `run_program` を通るので、`RunConfig` と出力の受け口を組み立てる場所は1か所のままである

## 対象外

- S3b-2 の項目 (上の「S3b の分け方」)
- 再帰の各段で別のエフェクトの handler を設ける形の `perform`。連鎖の長さが深さに比例するので、1回の `perform` の費用も深さに比例したままである。evidence passing ([evidence passing の設計](../../future/evidence-passing.md)) で解く
- 先頭に足す連結 (`"x" ++ acc`) と、共有された左辺への連結。どちらも左辺を写す
- ネイティブのオブジェクトモデル、ネイティブの不死の表現、記述子の形

## テスト

### 足すテスト

- `crates/eml_runtime/src/heap/tests.rs`
  - 不死の物体: `acquire_immortal` と `dup` と `decref` の数、数が 0 になっても解放しないこと、数が 0 のときの各操作の `UseAfterFree`、`is_unique` が偽であること、`take` が `Shared` を返し `take_or_copy` が写すこと、リークの報告に数を足すこと (1つのリテラルを2回手放し忘れると2)
  - `append_str`: 足したバイト数と結果の文字列
  - 連鎖: `copy_segment` が区間の中の `outer` と h の `inner` を写した側に付け替えること、区間の外を指す連鎖の参照が `BrokenSegment` になること
- `crates/eml_interp/tests/scaling.rs` (`tests/main.rs` で宣言する): 時間ではなく回数を比べるので、`#[ignore]` を付けずにふだんの `cargo test` で走らせる。各テストはテストの中でソースを生成し、n = 2000 で回数の上限を確かめる。今のコードではどれも2乗になって通らない

  | 形 | 確かめる回数 | 上限 |
  |---|---|---|
  | `Ask` の handler の下で、非末尾の `count n = ask () + count (n - 1)` | `handler_visits` | 2n |
  | 外に `Ask` の handler を置き、その内側に1つだけ `Tell` の handler を置き、その中で同じ再帰 | `handler_visits` | 3n |
  | `tests/ui/run/effects/mask_callback.em` と同じ形 (外の `Ask`、内の `Ask`、`mask` 付きで呼ぶコールバックの中で同じ再帰) | `handler_visits` | 4n |
  | リテラルから始めて `acc ++ "x"` を n 回つなぐ | `string_bytes_copied` | 4n |

### テストの変更

[テスト戦略](../../implementation/testing.md) の「テストの変更の運用」の種類で書く。

- 成否の変更 (単体テストの削除)
  - `eml_runtime` の `heap::tests::mark_shared_is_reserved_for_multicore` を削除する。`Heap::mark_shared` を削除するためである
  - `eml_runtime` の `output::tests::output_sink_is_send_and_sync` を削除する (補助関数 `assert_send_sync` も)。`OutputSink` を `Send + Sync` でなくすためである。`eml_cli/tests/api.rs` の `Session` の assert は残す
- 期待値の変更
  - `heap/tests.rs` のうち、`Frame::Mask` の値か handler のフレームの `link` の値を丸ごと比べるテストに、`outer` と `Attachment` が加わる。`Mask` を写すテストでは、期待する `outer` は写した側のフレームになる。handler の連鎖のフィールドを足すためである
- 機械的な追随
  - `live_objects_are_counted_by_descriptor` の名前を `live_objects_are_counted_by_kind` にする
  - `heap/tests.rs` で `Link`、`Frame::Mask`、handler のフレームを組み立てる箇所を、新しい形に合わせる
  - `RunStats` の戻り値に合わせて、`eml_cli` の `main.rs`、`tests/ui.rs`、`tests/api.rs` と `eml_test_support` を直す
  - `Arc<Program>` をやめることに合わせて、`eml_cli`、`eml_test_support`、各 crate のテストを直す
- 変わらないもの: UI テストの出力と、`eml_interp/tests/run.rs` のリークを報告するテスト

## 確認の手順

- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`
- 既定でない feature の組み合わせ: `cargo clippy -p eml_cli --no-default-features`、`--no-default-features --features types`、`--no-default-features --features core` と、`eml_test_support` の同じ組み合わせ
- `cargo test -p eml_cli --test integration citations` で、削除した見出しを指す参照が残っていないこと
- `nix build`

## 更新する文書

作業の途中で直すもの。

- `docs/spec/runtime.md`
  - 削除する: 冒頭の「予防的な決定もここにまとめる」、「マルチコアに備えた予防的な決定」の節と表、「API はマルチコア版と同じ形に固定する」と `mark_shared` の予約、「インタプリタとネイティブがオブジェクトモデルを共有するので並列の誤りを再現できる」、`OutputSink` の `Send + Sync` と `threads`、`schedule_seed` の予定
  - 書き直す: ヒープと参照カウント (`u32` の RC と不死の状態)、オブジェクトのヘッダ (記述子の代わりに種類の名前)、「記述子」と書いた箇所、`debug_heap` (不死のリテラルの数え方とその限界)、実行の API (`RunStats`)
  - 足す: 不死の物体の意味、一意な文字列のその場の連結、ランタイムの側から見た handler の連鎖 (`outer` と `inner` は所有せず、`children` はたどらず、`copy_segment` が付け替える)
  - 「ランタイムの API」と「実行の API」の見出しは残す。`core-ir.md` と `architecture.md` が参照している
  - 表を削除する前に、ほかの話題の行が別の文書にあることを確かめる (`Never ≤ Once ≤ Multi` の束は types.md と effects.md にある)
- `docs/spec/core-ir.md`: `Arc<Program>` で共有する文と、「CEK 機械の状態を `Send` にする」の文を削除する
- `docs/future/multicore.md`: 冒頭の runtime.md への参照、決定の表、「マルチコアのインタプリタ」の節を、インタプリタはシングルスレッドで並列化はネイティブだけ、という決定に合わせて整理する。自身の「マルチコアに備えた予防的な決定」の節は削除する。「可変状態」と「`par` の段階」の見出しは、ほかの文書が参照しているので残す。記述子が `Lin` の破棄処理を持つ話は、ネイティブの要件として書き直す
- `docs/future/roadmap.md`
  - 段の表と S3b の節を、S3b-1 と S3b-2 に分ける。S3b-2 の節に、この文書の「S3b の分け方」の S3b-2 の項目と、今の S3b の論点 (縮約パスの範囲、E0013 と深さ、別名の伝播と定数の畳み込み) を残す
  - 論点「ネイティブで包んだ `k` を `drop` したときの破棄処理」を S3b から外し、「その後の項目」の「記述子」の項目にまとめる。その項目の「S3b で名前しか持たない今の記述子を削除し」を、S3b-1 で削除した事実に直す
- `docs/overview.md` と `docs/README.md`: S3b への言及を S3b-1 か S3b-2 に直し、runtime.md の説明と「予防的な決定は spec/runtime.md に反映してある」の文を直す
- `docs/implementation/testing.md`: `eml_test_support` の関数の一覧に `run_stats` を足し、`core*` が `Program` を返すことにする。ヒープの単体テストの行に、不死の物体、その場の連結、連鎖の付け替えを足す。性能のテストの節に、回数を比べるテストが `#[ignore]` なしでふだんのテストに入ることを書く
- `CLAUDE.md`: `Send + Sync` の文を `Session` だけにし、理由をフロントエンドの側に直す。`eml_test_support` の説明で、`core*` が `Program` を返すことと `run_stats` を足す
- コードのコメント: `eml_core_ir/src/lib.rs` の `Arc<Program>` と `ConstString` (「実行のたびに新しい文字列をヒープに作る」)、`eml_interp/src/error.rs` の「記述子の名前ごとの数」、`eml_interp/src/lib.rs` の `RunConfig`、`eml_runtime` の `heap.rs`、`file.rs`、`output.rs`

段の終わりに1回で直すもの。

- `docs/implementation/architecture.md`: 「継続のフレーム」(連鎖、`Mask` の `outer`、`perform` のたどり方、連鎖の検査)、`eml_interp::run` と `eml_cli::execute` のシグネチャ、`Arc<Program>` の規約、実行の API で将来足すものへのリンク
- `docs/implementation/status.md`: 「深さと性能」に、この文書の「対象外」にある2つの限界 (各段で別のエフェクトの handler を設ける再帰と、先頭に足す連結) を書く
- `docs/future/roadmap.md`: S3b-1 の節を削除する

## 完了の条件

- UI テストの出力が1バイトも変わらない
- `scaling.rs` のテストが通る。`perform` がたどるのは連鎖のフレームだけで、決まった数の handler の下での非末尾の再帰は線形である。一意な左辺への `++` は、ならして右辺の長さに比例する
- 上の確認の手順がすべて通る
