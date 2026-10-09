# 型が守らない共有の印付けを実装で確かめる

先行実装はどれも、型 (または純粋性) で利用者のコードから競合を締め出し、その下にある実行時の仕組みを試験と検査道具で確かめる、という分担を取っている。ただし、eml と同じ符号付き RC を使う Lean 4 と Koka は、どちらも **共有の印付けの漏れを直接検出する仕組みを持っていない**。2026 年に両者で見つかった実バグは、RC の桁あふれと共有の組み合わせ、`make_shared` の 1 ずれ、共有 `ref` への格納での印付け漏れ、プロセス全体で共有する単一オブジェクトの 4 種類だった。これらは推論、RC の数え方の形式モデル、同じプログラムの反復実行で見つかっており、TSan では見つかっていない。理由は構造にある。RC の語をどちらの経路でも atomic として読み書きすると、Miri、TSan、loom の競合検出はその語の誤りを競合として報告しない。ここから推論として、eml は S9/S10 のシングルスレッド期のうちに次の 4 点を eml_rt に入れておくべきである。検査モードの所有者 ID、「共有から届くのは共有か不死だけ」という不変条件の走査、印付けの経路を逐次実行でも通す切り替え、loom と Miri を差し込める薄い抽象層である。並列 VM の項目では、逐次スケジューラの出力を基準 (serial elision) とし、乱数種付きの決定的スケジューラ、すべての子を別ワーカーへ移す強制モード、わざと印付けを外した負の試験を組み合わせる。決定的な並列の研究からは、例外の優先規則 (左の子が勝つ) を逐次と並列で最初から同じに定めることと、共有オブジェクトの再利用を Lean と Koka にならって禁止から始めることが導かれる。

本レポートでは、出典リンクを付けた記述を先行実装の資料で確かめられた事実として扱う。「推論」と明記した記述は、eml への当てはめを含む本レポートの推論である。

## 型が締め出すのは利用者の競合で、RC の印付けは実行時の義務として残る

OxCaml は保証の大半を型に置く。contention と portability という 2 つの mode の軸で「書き込みには uncontended が要り、読み出しには shared か uncontended が要る」という規則を課し、POPL 2025 の論文はデータ競合がないことを Iris/Rocq で機械化して証明した。その証明は、`Obj.magic` で実装した Capsule と RwLock の API も意味論的に型付けされていることまで含む ([Georges et al., POPL 2025](https://iris-project.org/pdfs/2025-popl-drfcaml.pdf))。実行時に残る強制点は mutex と atomic で ([OxCaml capsules](https://oxcaml.org/documentation/parallelism/capsules/))、メモリは GC が管理するので、ランタイムはオブジェクトが共有されているかを知る必要がない。調査ノートの推論として、これは eml との大きな違いになる。eml は Perceus の RC を使うので、非 atomic な RC の更新が競合するとメモリ安全性の不具合になる。そのため、送り手が公開の前に共有の印を付ける段階は、型と対になって保証を支える実行時の半分であり、OxCaml には対応するものがない。

OCaml 5 は逆に、型ではほとんど何も保証しない。データ競合があってもクラッシュせず、競合のない部分は逐次一貫に振る舞う (local DRF) という意味論を選び ([OCaml manual, Memory model](https://ocaml.org/manual/5.3/memorymodel.html))、継続の一回性は継続オブジェクトのスタック欄を CAS で取り出す動的検査で守る。ランタイムのコメントによれば、この CAS は他ドメインの GC のマーキングとの同期点も兼ねる ([runtime/fiber.c](https://github.com/ocaml/ocaml/blob/trunk/runtime/fiber.c))。Eio の README は、別ドメインに渡す関数がスレッド安全でない値に触れないようにするのは利用者の責任で、「The type system does not check this」と書いている ([Eio README](https://github.com/ocaml-multicore/eio))。

Lean 4 と Koka は eml に最も近い。Lean はオブジェクトの `m_rc` の符号で単一スレッド (正)、マルチスレッド (負)、永続 (0) を区別し、`lean_mark_mt` が到達可能な単一スレッドのオブジェクトを反復的な DFS で負に反転する。すでにマルチスレッドか永続のオブジェクトに当たると、その下へは降りない ([object.cpp L639-720](https://github.com/leanprover/lean4/blob/5e96ee1153f945234f126ed024043a0697c9d6f3/src/runtime/object.cpp#L639-L720))。この打ち切りは、論文が述べる「永続からは永続だけ、マルチスレッドからはマルチスレッドか永続だけに届く」という不変条件に依存する。タグを同期なしで読めるのは、タグが変わるのは値が他スレッドと共有される前だけだからだ、と論文は論じている ([Ullrich & de Moura, IFL 2019](https://arxiv.org/abs/1908.05647))。呼び出し元はタスクの生成、タスクと promise の解決、マルチスレッドの `ST.Ref` への書き込み、thunk の評価、明示的な API に限られる。コンパイラの IR には印付けの命令がない ([object.cpp L1157-1250](https://github.com/leanprover/lean4/blob/5e96ee1153f945234f126ed024043a0697c9d6f3/src/runtime/object.cpp#L1157-L1250))。Koka の kklib も同じ形で、`kk_block_mark_shared` を呼ぶのはタスクの投入、promise の設定、LVar、スレッド共有の `ref` への格納だけである ([thread.c L362-371](https://github.com/koka-lang/koka/blob/9c55695dd2f7d4db8d93011693d37295e2b76c53/kklib/src/thread.c#L362-L371))。Koka はタスク本体を total な関数に限るので、ハンドラや継続がスレッドをまたぐことはない ([lib/std/os/task.kk](https://github.com/koka-lang/koka/blob/9c55695dd2f7d4db8d93011693d37295e2b76c53/lib/std/os/task.kk))。

MPL は可変データを禁じれば階層ヒープの disentanglement が構成的に成り立つことを示し、可変データを許す場合は可変の参照外しだけを検査する動的検出を **5% 程度以下** の負荷で常時有効にした ([Westrick, Arora, Acar, ICFP 2022](https://www.cs.cmu.edu/~swestric/22/icfp-detect.pdf))。

| 系 | 型 (または純粋性) で保証すること | 実行時に残る仕組み | 正しさの主な確かめ方 |
|---|---|---|---|
| OxCaml | データ競合がないこと (mode) | mutex、atomic | Iris/Rocq による証明 |
| OCaml 5 | ほぼなし (競合しても安全な意味論) | 一回性の CAS、`Unhandled` | TSan、multicoretests |
| Lean 4 | 純粋性 | `mark_mt`、符号付き RC | RC 算術の形式モデル、TSan |
| Koka | タスク本体が total | `mark_shared`、符号付き RC | 小さなプログラムの反復実行 |
| MPL | 可変データなしなら disentangled | 可変操作での entanglement 検出 | 健全性と完全性の証明、性能評価 |

推論として、eml の `par` の子の row が `Never` の Kind に限られ、`Heap h` の操作が `run_heap` のスレッドでしか動かないという規則は、MPL の「可変データを禁じれば構成的に安全」、Koka の「タスク本体は total」と同じ位置にある。利用者のコードに対しては、Cilk 流の競合検出器が見つけるべきものは残らない。ところが RC では、言語の上では不変なオブジェクトでも、子の `dup` や `decref` はヘッダへの書き込みになる。eml で実装によって確かめる対象は、この印付けと、スケジューラと、例外の扱いである。

## 2026 年の実バグは、数え方の変換と公開経路の漏れで起きた

Lean では 2026 年に、RC の桁あふれと、桁あふれと共有の組み合わせで、use-after-free につながる不具合が続けて直された。PR #14838 は 32 ビットの桁あふれが公式カーネルで use-after-free を起こし得ることを示し、Koka にならった sticky な数を導入した ([PR #14838](https://github.com/leanprover/lean4/pull/14838))。PR #15288 は、単一スレッドの数があふれて sticky 帯に入ったオブジェクトが子に印を付けないまま共有されていた問題を直した。他スレッドが子の RC を非 atomic に更新し、使用中に解放できる状態だった ([PR #15288](https://github.com/leanprover/lean4/pull/15288))。PR #15241 は、解放の連鎖が sticky な数で止まらない問題を直している ([PR #15241](https://github.com/leanprover/lean4/pull/15241))。これらを支えたのは `tests/elab/rc_model.lean` で、inc、dec、`mark_mt` の数の更新を純粋な Lean の関数に写し、任意の inc/dec の列が理想的な自然数の数を詳細化すること、印付け後の数が二度と「未共有」と読めないことなどを `bv_decide` と `omega` で証明している。モデル自身が「どのオブジェクトを訪れるかはモデル化しない」と書いており、グラフの走査は証明の範囲外である ([rc_model.lean](https://github.com/leanprover/lean4/blob/5e96ee1153f945234f126ed024043a0697c9d6f3/tests/elab/rc_model.lean))。

Lean の CI は 2026 年 7 月に TSan のジョブを加えた。ただし TSan の下では `m_rc` を seq_cst の atomic で読み書きし、単一スレッドの速い経路が競合として報告されないようにしている ([lean.h L362-430](https://github.com/leanprover/lean4/blob/5e96ee1153f945234f126ed024043a0697c9d6f3/src/include/lean/lean.h#L362-L430); [PR #14161](https://github.com/leanprover/lean4/pull/14161))。調査ノートが `src/runtime` と `lean.h` を grep した範囲では、「マルチスレッドの子はマルチスレッドか永続」という不変条件を実行時に検査する debug assertion は見つかっていない。推論として、最も起きやすい不具合である印付けの漏れを、Lean の CI のどの道具も直接は検査していないことになる。

Koka の事例はさらに直接的である。2026 年 9 月 30 日の commit cf56076 は、`make_shared` が局所の数 N (参照 N+1 個) を `-N` (参照 N 個) に写していた 1 ずれを直した。回帰試験は、タスク側の drop が `xs` を解放したあとも `main` が読み続け、その間に `ys` の割り当てが同じセルを再利用する、という失敗を記録している ([commit cf56076](https://github.com/koka-lang/koka/commit/cf56076400); [test/lib/task-shared.kk](https://github.com/koka-lang/koka/blob/9c55695dd2f7d4db8d93011693d37295e2b76c53/test/lib/task-shared.kk))。前日の commit 046254c は、スレッド共有の `ref` に格納した値に印を付けていなかった不具合を直した。証拠は統計的で、修正前は **40 回中 40 回クラッシュし、修正後は 40 回中 40 回通った** ([commit 046254c](https://github.com/koka-lang/koka/commit/046254c139))。OS スレッドを async に入れようとした PR #910 は、2 スレッドの実行で **30-50% の割合でヒープが壊れた** と報告した。原因は空の evidence vector の単一オブジェクト、遅延初期化される文字列リテラル、計算されるトップレベル定数といった、プロセス全体のヒープブロックが非 atomic な RC を持っていたことである。この PR は統合されずに閉じられ、dev ブランチでは evidence vector の単一オブジェクトがまだ同期なしで遅延割り当てされる ([PR #910](https://github.com/koka-lang/koka/pull/910); [init.c L278-286](https://github.com/koka-lang/koka/blob/9c55695dd2f7d4db8d93011693d37295e2b76c53/kklib/src/init.c#L278-L286))。Koka の CI は `stack test --fast` で出力を比べるだけで、サニタイザーのジョブはない ([test.yaml](https://github.com/koka-lang/koka/blob/9c55695dd2f7d4db8d93011693d37295e2b76c53/.github/workflows/test.yaml))。

OCaml 5 の multicoretests は、ランダムな逐次の前置きと 2 ドメインでの並列の命令列を生成し、観測結果を説明する逐次の順序を探す。各ケースを既定で 25 回繰り返し、ランタイム、標準ライブラリ、Domainslib などで 40 件を超える不具合を見つけた。分析した 8 件のうち半分は逐次の試験で、残り半分は並列の試験で発覚した ([Midtgaard, OLIVIERFEST 2025](https://www.janmidtgaard.dk/papers/Midtgaard%3aOLIVIERFEST25.pdf))。GC の compaction の競合はデータ競合ではなく論理的な競合で、TSan のような開発者向けの道具では捕まらない種類だったと論文は明記している (同上)。

推論として、実バグは 3 つの型に分かれ、それぞれ確かめ方が違う。1 つめは局所と共有の数の変換と桁あふれの算術で、これは印付けの経路を通しさえすれば単一スレッドでも検出できる。Koka の 1 ずれは、印を付けた直後に送り手が使い続けて割り当てを行うだけで解放後の使用になるので、eml の debug_heap の poisoning で捕まる種類である。2 つめは公開経路の漏れで、プリミティブごとの網羅表と、公開時の根の assertion で防ぐ。3 つめはランタイム全体で共有するオブジェクトで、生成時から不死にすることで防ぐ。

## どの検査道具も、印付けの漏れを直接は報告しない

Rust の検査道具は、それぞれ見える範囲が違う。loom は C11 のメモリモデルの下で実行の順序をほぼ網羅的に並べ替えるが、`loom::sync::atomic`、`loom::cell::UnsafeCell`、`loom::alloc` を通した操作しか見えない。SeqCst のアクセスは AcqRel として扱い、load buffering は探索しないので健全ではない。preemption の上限は 2 か 3 で、ほとんどの不具合に足りるとされる ([loom docs.rs](https://docs.rs/loom/latest/loom/); [loom README](https://github.com/tokio-rs/loom))。loom の `UnsafeCell` は vector clock で因果関係を確かめ、同時の読み書きを「Causality violation」として panic する ([loom src/rt/cell.rs](https://github.com/tokio-rs/loom/blob/master/src/rt/cell.rs))。tokio はすべての同期プリミティブを `cfg(loom)` で差し替える層を持ち、loom 専用の CI を `LOOM_MAX_PREEMPTIONS: 2` で分割実行している ([tokio loom.yml](https://github.com/tokio-rs/tokio/blob/master/.github/workflows/loom.yml))。shuttle はランダムと PCT のスケジューラで大きな試験まで扱えるが、すべての atomic 操作を SeqCst として扱うので、弱いメモリ順序の不具合は見つけられない ([shuttle atomic](https://github.com/awslabs/shuttle/blob/main/shuttle-std/src/sync/atomic/mod.rs))。AWS の ShardStore は、小さく重要な並行プリミティブには loom、大きな試験には shuttle と使い分けている ([ShardStore SOSP'21](https://www.cs.utexas.edu/~bornholt/papers/shardstore-sosp21.pdf))。

Miri は vector clock による競合検出、場所ごとのストアバッファによる弱いメモリの一部の模倣、乱数種で決まるスケジューリングを備える。非 atomic な書き込みと atomic なアクセスの混在は競合として報告するが、ネイティブの **約 3000 倍から 7000 倍** 遅い ([Jung et al., POPL 2026](https://research.ralfj.de/papers/2026-popl-miri.pdf))。TSan は最適化したバイナリを **5-15 倍** の速度低下で走らせ ([Clang ThreadSanitizer](https://clang.llvm.org/docs/ThreadSanitizer.html))、Rust では nightly の `-Zsanitizer=thread` と `-Zbuild-std` で使う。aarch64-apple-darwin も対象に含まれる。`atomic::fence` は理解しないので、標準ライブラリの `Arc` は TSan の下でだけ fence を acquire の load に置き換えている ([Rust unstable book](https://doc.rust-lang.org/beta/unstable-book/compiler-flags/sanitizer.html); [arc.rs](https://github.com/rust-lang/rust/blob/main/library/alloc/src/rcs/arc.rs))。crossbeam は Miri (strict provenance と Tree Borrows の行列)、ASan/MSan/TSan、loom を CI で回す。Chase-Lev の deque はスロットの読み書きに volatile を使った意図的なデータ競合を含み、CI はそれを抑制ファイルと Miri のフラグで避けている ([crossbeam ci/miri.sh](https://github.com/crossbeam-rs/crossbeam/blob/master/ci/miri.sh); [crossbeam-deque deque.rs](https://github.com/crossbeam-rs/crossbeam/blob/master/crossbeam-deque/src/deque.rs))。

印付けの漏れに直接効く先例は、所有者を記録する仕組みである。CPython の free-threading はヘッダに所有スレッドの `ob_tid` を持ち、所有スレッドは非 atomic、他スレッドは atomic で RC を更新する ([PEP 703](https://peps.python.org/pep-0703/))。`fragile` crate は、別スレッドへ送られた値に触れたり drop したりすると実行時に失敗させる ([fragile README](https://github.com/mitsuhiko/fragile))。

推論として、eml_rt の局所経路は `AtomicI32` の Relaxed な load/store なので、印を付け忘れた値を 2 スレッドが更新しても、言語のメモリモデルの上ではデータ競合ではない。Miri、TSan、loom のどれも、これをデータ競合としては報告しない。誤りは数の取りこぼしによる早すぎる解放か、リークとして間接的に現れるだけである。Perceus の再利用があると、古い `rc == 1` を見た所有者がフィールドを非 atomic に書き換えるので、ここで初めて Miri、TSan、loom のどれもが競合を報告できる。下の表は調査ノートの推論で、測定ではない。

| 不具合の種類 | 所有者 ID の assertion | loom | shuttle | Miri | TSan |
|---|---|---|---|---|---|
| 印付けの漏れ (RC の語だけ) | 最初の誤ったアクセスで確実に捕まる | モデル化すれば数の誤り、二重解放、リークとして | 順序に当たれば | 乱数種しだいで間接的に | 報告しない |
| 印付けの漏れと再利用やフィールド書き込み | より早く捕まる | `UnsafeCell` のフィールドで報告 | assertion があれば | 報告 | 報告 |
| 公開時の順序の誤り (Release のはずが Relaxed) | 捕まらない | 見つけられる | 見つけられない | 弱いメモリの模倣で一部 | 後続のフィールド競合として |

所有者 ID は、通常の `cargo test`、loom、shuttle、Miri、TSan のどの実行でも、スケジュールに依らない panic に変えられる点で他の道具と性質が違う。所有者の欄そのものは Relaxed の atomic にしないと、検査自体が競合として報告される (CPython も `ob_tid` を relaxed atomic で読む)。

## 逐次実行を基準にすると、決定的な並列を試験できる

決定的な並列の研究は、決定性を主に証明と型で得ている。monad-par は `runPar :: Par a -> a` の純粋性と書き込み一回の IVar で決定性を論じ、評価は性能だけだった。同じ `Trace` の表現を逐次と work-stealing の 2 つのスケジューラが消費する構成で、子で ⊥ が起きた場合を逐次スケジューラは正しく扱うが、並列スケジューラはおそらくデッドロックになると論文自身が書いている ([Marlow, Newton, Peyton Jones, Haskell 2011](https://simonmar.github.io/bib/papers/monad-par.pdf))。LVish は決定性の水準を `Par Det` と `Par QuasiDet` の型で区別し、衝突する書き込みはどの順序でも束の頂上に達するので、エラーそのものが決定的に起きる ([Kuper et al., POPL 2014](https://users.soe.ucsc.edu/~lkuper/papers/lvish-popl14.pdf))。ライブラリには、デバッグ点でスレッドの交互実行を制御する `dbgScheduling` がある ([Control.LVish](https://hackage.haskell.org/package/lvish-1.1.4/docs/Control-LVish.html))。

試験の基準を先例の中で最も明確に定めているのは Cilk の serial elision である。OpenCilk は決定的なプログラムの振る舞いを、spawn と sync を消した逐次プログラムで定義する ([OpenCilk spec](https://cilk.mit.edu/docs/OpenCilkLanguageExtensionSpecification.htm))。Nondeterminator の SP-bags は逐次実行のまま、与えた入力に対して **すべてのスケジュールが同じ振る舞いになること** を確かめ、負荷は最適化コードの 12 倍未満である ([Feng & Leiserson, SPAA 1997](https://homes.cs.washington.edu/~mernst/teaching/6.893/readings/feng-spaa97.pdf))。例外については、Rayon の `join` は 2 つのクロージャを必ず両方実行し、両方が panic したら最初のクロージャの値で panic する ([rayon::join](https://docs.rs/rayon/latest/rayon/fn.join.html))。OpenCilk も逐次順で最も早い例外を投げる。一方 Java の `invokeAll` は「any one」を投げ、他を取り消すこともある ([Java 21 ForkJoinTask](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/util/concurrent/ForkJoinTask.html))。

スケジュールの再現性については、FoundationDB がクラスタ全体を単一スレッドの決定的なシミュレーションで動かし、決定性を「perfect repeatability」のために不可欠としている ([FoundationDB testing](https://apple.github.io/foundationdb/testing.html))。shuttle は失敗したスケジュールを文字列として出力し `replay` で再生でき ([shuttle docs.rs](https://docs.rs/shuttle/latest/shuttle/))、Miri も乱数種でスケジュールを決める。

推論として、eml の `par` は monad-par を spawn だけに絞った形に近く、子は閉じた計算で、出力は戻り値と `Never` の Kind の操作だけである。そのため利用者の決定性は型で決まり、試験の対象は実装になる。逐次 VM がそのまま serial elision なので、UI 試験の出力スナップショットを基準にすれば、並列スケジューラの出力がそれと一致するかを確かめられる。例外の優先規則を左の子と決めるなら、左の例外は右を取り消してよいが、右の例外は左が終わるか例外を出すまで待つ、という非対称な扱いだけが逐次と一致する。左の子が停止しないときに右の例外を早く報告すると逐次と食い違うが、UI 試験は停止するプログラムしか持たないので、この食い違いは試験では見えない。規則として仕様に書いておく必要がある。

## eml への提案

### S9/S10 のシングルスレッド期に入れておくこと

以下はすべて推論である。並列化の時点で検証できるようにするための備えで、シングルスレッドの性能にはほぼ影響しないものを選んだ。

| 優先 | 項目 | 根拠となる先例 | 確かめ方 |
|---|---|---|---|
| 1 | 検査モードのヘッダに所有者 ID (Relaxed の `AtomicU32`) を持たせ、正の RC への `dup`、`decref`、フィールドの書き込み、再利用で現在のワーカーと一致することを assert する。今は常にワーカー 0 で、配線だけを先に通す | CPython の `ob_tid`、`fragile`、Lean と Koka に検査がなかったこと | UI 試験の debug_heap 実行で常に有効 |
| 1 | 検査モードで「共有から届くのは共有か不死だけ」を走査する。印付けの直後と、割り当て台帳の生存オブジェクト列挙のときに行う | Lean の不変条件、Koka の commit 046254c の説明 | 台帳の列挙に組み込む |
| 1 | ランタイム全体で共有するオブジェクト (CodeBase の定数、文字列リテラル、空の evidence) を生成時から不死 (RC 0) にし、遅延初期化は once で行う | Koka PR #910 の 30-50% のヒープ破壊 | 検査モードで「CodeBase から届くものは不死」を assert |
| 1 | eml_rt の atomic、cell、割り当てを薄い抽象層の裏に置き、`cfg(loom)` で差し替えられるようにする。核では SeqCst のアクセスと `fence` を使わない | tokio の `src/loom/mod.rs`、loom の SeqCst の扱い、TSan が fence を理解しないこと | `cargo test` と後の loom 実行で同じコードを使う |
| 2 | `mark_shared` と負の RC の経路を S9 で実装し、eml_rt の単体試験と Miri (strict provenance) で確かめる。加えて、検査モードだけで `par` の逐次実装が子に渡す値へ印を付ける切り替えを検討する (合意済みの制約 3 の変更になる) | Koka の 1 ずれは単一スレッドの解放後使用として現れたこと | debug_heap の poisoning、Miri |
| 2 | RC の桁あふれ (sticky 帯) の規則を S9 で決め、理想的な自然数の数と比べる性質試験で確かめる | Lean の 2026 年の 3 件の修正と `rc_model.lean` | proptest などで任意の inc/dec/印付けの列を生成 |
| 2 | 再利用の判定 (`is_unique`) は負の数に対して常に偽にする | Lean の `lean_is_exclusive`、Koka の `kk_block_drop_reuse` | 検査モードで再利用対象の所有者と正の RC を assert |
| 2 | `par` の逐次実装を基準の意味論とし、左の子の例外が勝つこと、停止しない左の子と右の例外の扱いを仕様に書き、逐次で UI 試験にする | monad-par の並列スケジューラの不一致、Rayon、OpenCilk | `run-fail` の UI 試験 |
| 3 | `CodeBase: Sync` と、VM の状態にスレッドローカルがないことをコンパイル時の assert で固定する (`Session` の `Send + Sync` の試験と同じ形) | Lean のインタプリタがスレッドごとに状態を持ち、共有するキャッシュを mutex で守ること | `crates/*/tests` の型の assert |
| 3 | S7 で継続がヒープのクロージャになるとき、捕捉する evidence が `Ctx` へのポインタを持たないことを Core IR かバイトコードの検証器で確かめる | Koka の調査ノートの推論 (Koka はこの問題を避けており、解いていない) | 検証器の規則 |

### 並列 VM の項目で作る検査と試験

並列 VM の項目が `par` の a1 だけから始まる前提で、優先順に並べる。これもすべて推論である。

| 優先 | 項目 | 根拠となる先例 |
|---|---|---|
| 1 | 乱数種付きの決定的スケジューラ。N 個の論理ワーカーを 1 スレッドで動かし、セーフポイントで切り替える。失敗時に種を出力して再生できるようにし、所有者 ID は論理ワーカーで記録する | FoundationDB、shuttle の `replay`、LVish の `dbgScheduling` |
| 1 | すべての `par` の子を別ワーカーへ移す強制モード。印付けと受け渡しの経路を、単一コアで確実に通す | 調査ノートの推論 (Cilk の serial elision と MPL の検出の組み合わせ) |
| 1 | UI の `run` と `run-fail` の全試験を、逐次、N ワーカーの並列、乱数種付きの 3 通りで実行し、出力が逐次のスナップショットと一致することを確かめる。RunStats のうち `peak_objects` はスケジュールに依るので、ベンチのスナップショットは逐次だけで固定する | OpenCilk の serial elision |
| 1 | 負の試験。検査用のフラグで印付けを 1 か所外したとき、上の試験のどれかが必ず失敗することを確かめる | multicoretests の `neg_agree_test_par` |
| 2 | loom のモデル。印付けと公開、両側からの `dup`/`decref`、最後の解放、`par` の join の手順を 2-3 スレッドで書く。オブジェクトのフィールドは loom の `UnsafeCell`、割り当ては `loom::alloc` を通し、`LOOM_MAX_PREEMPTIONS=2` で回す | tokio、crossbeam-epoch、ShardStore |
| 2 | Miri で eml_rt の並行の単体試験を strict provenance と many-seeds で回す。VM 全体は小さなプログラムだけにする | Miri の速度と検出率、crossbeam の CI |
| 2 | TSan (`-Zsanitizer=thread`、`-Zbuild-std`、`--release` と debug assertions) で VM に `par` の UI 試験を走らせる。RC の語の誤りは見えず、フィールドの競合として結果だけが見えることを前提にする | Lean の TSan ジョブ、Rust の TSan の制約 |
| 3 | work-stealing の deque を自作するなら、スロットを `AtomicPtr` か `AtomicUsize` にして Chase-Lev の意図的な競合を避ける。あるいは private deque にして、所有者が盗みの要求に応える時点で印を付ける | crossbeam-deque、Acar, Charguéraud, Rainey の private deque |
| 3 | 試験ごとに新しいプロセスとタイムアウトで動かす長時間のストレス試験。クラッシュと debug_heap の不変条件だけを見る | multicoretests の `stress_test_par` と fork |

private deque については、各プロセッサの deque が非公開で、仕事はメッセージで受け渡し、局所操作に fence が要らないことが示されている ([Acar, Charguéraud, Rainey, PPoPP 2013](http://www.chargueraud.org/research/2013/ppopp/full.pdf))。推論として、仕事がワーカーを離れる時点が所有者自身の応答に限られるので、盗まれた子だけに印付けの費用を払わせられる。

## 先行実装から出てくる設計上の問い

共有オブジェクトの再利用を許すかが最初の問いである。Lean の `lean_is_exclusive` はマルチスレッドのオブジェクトに対して常に偽を返し ([lean.h L776-794](https://github.com/leanprover/lean4/blob/5e96ee1153f945234f126ed024043a0697c9d6f3/src/include/lean/lean.h#L776-L794))、Koka も共有で唯一 (-1) のオブジェクトを再利用せず、atomic に drop して解放する ([refcount.c L216-235](https://github.com/koka-lang/koka/blob/9c55695dd2f7d4db8d93011693d37295e2b76c53/kklib/src/refcount.c#L216-L235))。推論として、共有オブジェクトを atomic に読んで再利用する計画は先例より踏み込んでおり、少なくとも acquire の読み出しと、それを済ませてからの局所への戻しが必要になる。並列の安全性を確かめることを優先するなら、まず禁止で始め、性能の必要が出てから検証の手段と一緒に入れるのがよい。関連して、Lean はマルチスレッドから単一スレッドへ戻す経路を持たず、Koka は最後の共有の drop で 0 に戻す ([refcount.c L192-214](https://github.com/koka-lang/koka/blob/9c55695dd2f7d4db8d93011693d37295e2b76c53/kklib/src/refcount.c#L192-L214))。推論として、eml では join の後、子の結果が共有のまま親に戻るので、親側の再利用が効かなくなる。join の時点で全子が終わっていることを使って局所へ戻せるかは、検証の対象を増やす代わりに得るものがある問いである。

2 つめは、`par` の子に渡す値をいつ、どこまで印付けするかである。Lean の論文はタスク生成の費用が到達可能な単一スレッドの値の数に比例すると認め ([Ullrich & de Moura](https://arxiv.org/abs/1908.05647))、保守者はタスクに捕捉された排他的なオブジェクトまで印を付けてしまう問題 ([issue #885](https://github.com/leanprover/lean4/issues/885)) と、thunk の結果を無条件に印付けする問題 ([PR #15047](https://github.com/leanprover/lean4/pull/15047)) を未解決のまま抱えている。推論として、eml の `Lin` の Kind を持つ値は子へ移動するだけなので、印付けの代わりに所有者の移し替えで済む可能性がある。そうするなら、所有者 ID の assertion がその移し替えを検査する手段になる。印付けを fork の時点で行うか、盗まれた時点で行うかは、どちらを選ぶかで loom のモデルの形が変わる。

3 つめは読み出しだけの共有である。OxCaml は shared と shareable という格子の点を足し、`fork_join2` の子が環境を読めるが書けないようにした ([janestreet/parallel](https://github.com/janestreet/parallel))。推論として、`Heap h` の get が再開する操作なら、eml の `par` の子は親の `Ref` を読むことすらできない。表現力の不足として現れたときに広げるなら、OxCaml がしたように、API ごとに意味論的な健全性を示す手段を先に用意しておく必要がある。

4 つめは実行時エラーの決定性である。推論として、左の子の例外が勝つ規則に加えて、extern 呼び出しの失敗が出す `at path:line:column` や、debug_heap が報告するリークと不変条件の違反も、スケジュールに依らず同じ内容で出る必要がある。そうでないと `run-fail` のスナップショットが並列の実行で揺れる。左の子が失敗したあと、まだ動いている右の子が出す失敗の報告や、右の子を取り消すときの後始末をどう扱うかは、先例の資料からは答えが出ない。OpenCilk は、逐次順で後の例外オブジェクトを未規定の順序で破棄すると定めるだけである ([OpenCilk spec](https://cilk.mit.edu/docs/OpenCilkLanguageExtensionSpecification.htm))。

5 つめは、後回しにした継続の移送である。OCaml 5 のランタイムは他ドメインでの再開を許し (マニュアルに明文の規定はない)、一回性の CAS が他ドメインとの同期点も兼ねる ([runtime/fiber.c](https://github.com/ocaml/ocaml/blob/trunk/runtime/fiber.c))。OxCaml の `handled_effect` は unique な継続を portable にし、10 個のドメインで順に再開する試験を持つ ([handled_effect test/portable.ml](https://github.com/janestreet/handled_effect/blob/oxcaml/test/portable.ml))。推論として、eml の `once` は一回性の動的検査を不要にするが、受け渡しには release と acquire の組が必要で、TSan の下で同期の辺が見えるように明示的な atomic を通すべきである。

## 結論

先行実装を並べると、「型で安全」と「実装が正しい」の間には、RC を使う言語に特有の隙間があることが分かる。OxCaml の証明は GC の上で成り立ち、Lean の証明は数の算術だけを扱い、Koka の確かめ方は反復実行である。そのため、eml が並列の安全性を実装で確かめるなら、まず検査モードを強くする必要がある。具体的には、所有者 ID、不変条件の走査、印付けの経路を逐次で通す切り替えで、印付けの漏れをスケジュールに依らない失敗に変える。loom、Miri、TSan は、その上で順序とメモリモデルの誤りを探す二段目にあたる。

案 B は、この順序と噛み合っている。シングルスレッドの S9/S10 で入れる備えは、ほとんどが「今は自明に成り立つ assertion と、差し替え可能な層」で、並列 VM の項目ではそれを有効にするだけで最初の検査がそろう。逆に、共有オブジェクトの再利用や、join 後に局所へ戻す最適化は、検証の手段がそろうまで入れない方が、確かめられる範囲を広く保てる。
