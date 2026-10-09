# eml: REPL 向けバイトコード VM のランタイム設計と、ネイティブコードとの共通化の調査

対象は commit `97277f19a4d7f514dd64008af3c1391507c52322` の `eml` である。コードの引用はこのコミットの行番号で書く。「検証済み」はコードを読むか実行して確かめたこと、「推測」は読んだ事実からの私の推論、「資料の主張」は論文や公式資料の記述を指す。計測と実験に使ったコピーの差分と結果は `experiments/` に置いた (`results.md` に一覧がある)。マウントされたコードは変更していない。

## 要約

### 問1: 現在のランタイムを再設計すべきか

再設計する。ただし全部を作り直すのではなく、次の三つに分ける。

1. **参照実装として凍結する部分**。`crates/eml_runtime` の検査付きヒープと、CEK (`machine.rs`、`runtime.rs`、`effects.rs`) である。新機能は足さず、差分テストの基準として使う。REPL のために手を入れるのは、所有する長寿命のランタイム、割り込み、異常終了後の回復、セッションの根の四点に限る。
2. **新しく作る共通ランタイム** (仮称 `eml_rt`)。8 バイトの語 (Word) を値とし、ヘッダ付きのオブジェクトと記述子、RC、extern の語 ABI、実行文脈 `Ctx`、検査モードを持つ。VM とネイティブが使う。
3. **新しく作る VM**。evidence passing を通した lowered Core から作り、共通ランタイムの上で動く。

REPL に固有の要件 (入力ごとの増分、セッションをまたぐ状態、異常終了からの回復、Ctrl+C、値の表示) は、現行のランタイムを改修すれば満たせる。この点の確度は高い。一方、VM の値を現行の 16 バイトの `Value` enum と安全ヒープに載せたままにすると、後でネイティブコードや JIT と同じヒープを共有できなくなる。この点の確度は中から高である。ここが前回の結論からの最大の変更点になる。

### 問2: ネイティブと共通化すべきか

共通化する。ただし層ごとに範囲を決める。

| 境界 | 共通化 | 共通にするもの | 分けるもの | 確度 |
|---|---|---|---|---|
| IR | する | lowered Core (evidence passing 後、Perceus 後)、所有規則 | 命令の実行 (バイトコードか機械語か) | 高 |
| ABI | 論理だけ | Repr ごとの引数と戻り値、T3、`f$boxed`、引数は所有の規約、関数表 `FnEntry` | レジスタ割り当て、スタック枠の形 | 中 |
| 値の表現 | する | 語 (Word)。`obj` と `tobj` は「ポインタか即値」、`int` と `enum` と `unit` は型付きの生のスロット | CEK の `Value` enum (参照実装) | 中から高 |
| ヒープ | する | オブジェクトのヘッダ、種類ごとの記述子、確保と解放 | 検査付きの世代番号ヒープ (テスト用) | 中 |
| RC | する | `dup` と `decref` と `release_fields` の意味、カウンタの符号の規約 | 複数スレッドの原子的な RC (ネイティブだけが使う) | 中 |
| 継続とエフェクト | Core の変換として | evidence passing、yield を見る規約、`Ctx` | CEK のヒープ上のフレーム | 中から高 |
| extern | する | `eml_extern` の表と、語 ABI で一度だけ書く実装 | CEK 側の実装 (当面は別に持つ) | 中 |

共通化の根拠は次のとおりである。Lean の IR インタプリタは、ネイティブのコードがある関数ではそちらに切り替える ([Lean 4 の論文](https://lean-lang.org/papers/lean4.pdf)の抜粋)。GHCi は、[コンパイル済みのコードとバイトコードを同じ実行系で混在させる](https://downloads.haskell.org/ghc/latest/docs/users_guide/ghci.html)。この二つでは、解釈されるコードの値がネイティブと同じヒープの上にあると考えられる (推測)。一方、OCaml はバイトコードとネイティブの混在を許さず ([マニュアル](https://ocaml.org/manual/5.0/native.html))、ネイティブのトップレベルを別に作った。混在を目標にするなら値とヒープの表現を一致させる必要がある、というのが私の読みである。

### 前回の結論の見直し

前回の結論のうち、次の点は今回の観点 (REPL とネイティブとの共通化) から見直すべきである。根拠は本文にある。

| 前回の結論 | 今回の判断 | 確度 |
|---|---|---|
| 値の表現はまず 16 バイトの `Value` enum を保つ | VM とネイティブを同じヒープで混在させるなら、VM を作る前に語表現へ移る。CEK だけが `Value` を保つ | 中から高 |
| 第 1 段は `Runtime` を再利用する | `Runtime<'p>` は 1 回の実行のためのもので (`crates/eml_interp/src/runtime.rs:92`)、そのままでは REPL に使えない。先に所有型へ直す必要がある。第 1 段の VM はネイティブと混在できない使い捨てになる | 高 |
| `saved` を IR から外して派生情報にする | 派生情報にするのはよい。ただし中断時の解放と継続の捕獲に同じ情報が要る。VM の呼び出し位置ごとの側表として実行時に残すか、回復を根からの走査にして不要にするかを選ぶ | 中 |
| `box` と `unbox` はレジスタの別名として扱う | CEK の `Value` では成り立つが、語表現では 63 ビットに収まらない `Int` を確保する命令になる | 中 |
| evidence passing は VM と同時か直後 | REPL とネイティブの混在が目的なら、VM より前に Core から Core への変換として作る | 中から高 |
| CEK は参照実装として残す | 賛成する。加えて、REPL のセッション意味論の最初の実装を CEK の上に作ることを勧める | 高 |
| VM は lowered Core から直接レジスタ型バイトコードを作る | 維持する。レジスタは Repr ごとに型を持たせ、`int`、`enum`、`unit` は生のスロット、`obj` と `tobj` は語にする。関数表 `FnEntry` と `Ctx` を前提に加える | 中から高 |

### 実験で分かった主な点

- CEK で実行時エラーが起きると、ヒープにフレームと退避した値が残る。深さ 1000 の再帰の底で `1 / 0` を起こすと、Frame 1002 個と String 1000 個が残った。継続の連鎖を `decref` すると、フレームと退避値は全部解放されたが、誤りを起こした関数の局所変数の String が 1 個残った (E3、検証済み)。セッションをまたぐヒープでは、この残りが溜まる。
- 割り込みの確認は `Machine::transfer` の 1 か所で足りる。ループの辺が無い (R2) ので、無限に走るプログラムは必ず呼び出しか戻りを通るためである。確認を入れても `fib 30` は 1.033 秒から 1.048 秒、1000 万回のループは 2.896 秒から 2.822 秒で、揺れの範囲だった。300 ミリ秒後に割り込むと 0.303 秒で止まった (E4、E5、検証済み)。
- 非末尾の無限再帰は、1 秒で最大 RSS が約 582 MB になった。CEK はフレームをヒープに置くのでスタックオーバーフローを起こさず、メモリが尽きるまで走る。REPL には Ctrl+C のほかに資源の上限が要る (E4、検証済み)。
- 入力ごとにプログラム全体を処理し直す費用は小さい。`main () = println "hi"` で 2.4 ms、関数を 1000 個足しても 37 ms だった (E2)。したがって増分コンパイルは最初の必須条件ではない。一方、ヒープの `Closure.function` と `Value::Fn` は関数番号 (`FnIdx`) をそのまま持ち (`crates/eml_runtime/src/heap.rs:23`、`:68`)、`FnIdx` は変換のたびに振り直される (`crates/eml_core_ir/src/translate/program.rs:167`)。ヒープに値が残るセッションでは、関数表を追記だけの表にすることが必須である (検証済み)。

RC の系譜 (Lean から Perceus まで) は、「技術的背景と関連研究」で全文に基づいて実装と比べた。eml の `release_fields` などは論文の drop specialization に当たり、実行時エラーと割り込みは Perceus の規則の外にあることが、回復の設計を決める根拠になった。

移行は、基準の整備、CEK の上の REPL セッション、evidence passing、共通ランタイム、VM、ネイティブの順にする。各段階の検証は「改善提案」の末尾の表に置いた。

## 調査結果

### 1. 現行ランタイムの構成と、各部分の判断 (検証済み)

`eml_interp::run` は呼ばれるたびに `Machine` を作り、その中で `Runtime` と `Heap` を新しく作る (`crates/eml_interp/src/lib.rs:60`、`crates/eml_interp/src/machine.rs:35`)。ヒープは 1 回の実行で捨てられる。

| 部品 | 現状 | REPL とネイティブから見た判断 |
|---|---|---|
| `Value` (`heap.rs:17`) | 16 バイトの `Copy` enum。`Int(i64)`、`Unit`、`Tag(u32)`、`Fn(u32)`、`Obj(ObjRef)` | 自分の種類を持つので `box` と `unbox` が空になり、`Repr` を読まずに済む。語表現とは互換でない。CEK に残す |
| `ObjRef`、`Slot`、`Header` (`heap.rs:10`、`:156`、`:166`) | 添字と世代番号の組。`Slot` は 96 バイト (E1)。世代番号の不一致で解放済みを検出する | 安全で検査は無料に近い。ネイティブのコードが直接読める形ではない。検査モードの参照実装として残す |
| `Payload` (`heap.rs:29`) | Rust の enum。`Data` は `Vec<Value>` を持つので、コンストラクタ 1 個ごとにスロット (96 バイト) とは別にフィールド配列の確保が 1 回ある。`Frame` と `Continuation` もヒープの種類である | ネイティブには使えない (Rust の enum の配置は不定)。共通ランタイムでは、evidence passing の後に `Frame`、`Continuation`、`Handler` の種類が要らなくなる |
| `Payload::Data` (`heap.rs:43`) | タグとフィールドだけで、配置の ID を持たない | 値が自分の型を持たない。表示には静的な型が必要になる (下の「値の表示」) |
| `Frame`、`Attachment` (`heap.rs:75`、`:111`) | CEK のフレーム。`Return` は `saved` の `Vec` を持つ。handler の連鎖は所有しない `outer` でつなぐ | CEK 専用。VM とネイティブには持ち込まない |
| `Heap::decref_all` (`heap.rs:288`) | 作業リストで子をたどる。呼び出しごとに `Vec` を作る | 流用できる考え方。共通ランタイムでは確保なしの作業スタックにする |
| `Heap::live_objects` (`heap.rs:514`) | 種類ごとの生存数を数える。`slots` は縮まない (`truncate` と `shrink` は無い) | 異常終了後の診断に使える。縮まないので、暴走した実行の後もメモリが戻らない |
| `Runtime<'p>` (`runtime.rs:92`) | `strings: &'p [String]`、`out`、`file_root` を借りる。`literals` は `Runtime::new` で作る。`arities` は `Machine::new` で `Program` から作る (`machine.rs:36`) | 寿命が 1 回の実行に縛られる。所有型にして、関数表と文字列表を追記だけにする必要がある |
| `Env` (`runtime.rs:42`) | 関数に入るたびに `Vec<Option<Value>>` を確保する | 確保を除くと速くなる (前回の提案 1)。VM では連続したレジスタ枠に置き換わる |
| `effects.rs` | `find_handler` が handler の連鎖をたどる (`:11`)。`perform` は継続の区間を切り出し (`:58`)、`resume` は `take_or_copy` で区間を写す (`:109`)。連鎖が `Root` に届くと内部エラーにする (`:42`) | CEK 専用。VM とネイティブは evidence passing に置き換える |
| `externs.rs` | `call_extern` が `Value` と `Heap` に直接書かれた約 180 行。`Eq` と `Ne` は到達しない (`:108`) | 語 ABI で一度だけ書く対象。S4 で `String` の extern が増える前に決める |
| `RunConfig`、`RunStats`、`run` (`lib.rs:18`、`:43`、`:60`) | 1 回の実行の設定と統計 | セッションの設定と統計 (`RunStats` は入力ごとの差分) に広げる |

設計文書は既にインタプリタとネイティブのランタイムを分けている。`docs/spec/runtime.md:5` は `eml_runtime` を「インタプリタのためのシングルスレッドのヒープ」とし、`docs/future/multicore.md:34` は今のヒープを「検査付きヒープとして残す」と決めている。今回の提案はこの分け方を変える。VM をネイティブと同じ側に置くからである。したがって、VM に着手する前に `runtime.md` と `multicore.md` を改める作業 (spec の更新) が要る。

### 2. REPL の要件ごとの検討

#### 2.1 入力ごとの増分コンパイルと、定義の追加と再定義

変換は入口の関数から届く関数だけを変換し (`crates/eml_core_ir/src/translate/mod.rs:35` の `reachable`)、関数番号は `reserve` で順に振る (`translate/program.rs:167`)。同じ関数でも入口が違えば番号が違う。ヒープの `Closure` は `function: u32` を持ち、関数定数 `Atom::Fn` は `Value::Fn(u32)` になる。文字列の定数は `Program.strings` の添字で引き (`crates/eml_core_ir/src/lib.rs:27`)、配置は最初に使った順に振られる (`docs/spec/core-ir.md:130`)。エフェクトの番号は HIR の `EffectId` に従う。

したがって、入力のたびに `lower` を呼び直して新しい `Program` を作る方式は、ヒープに値が残る間は使えない。残った `Closure` の関数番号が別の関数を指すからである。必要なのは、関数、文字列、配置、エフェクトを追記だけにした表 (仮称 `CodeBase`) である。

ロードマップは、後の入力で定義し直しても変わるのはそれより後の入力だけとし (`docs/future/roadmap.md:193`)、定義ごとに Core IR を保存し、後の定義で変わる ABI は `f$boxed` を足すかどうかだけにしている (`docs/future/roadmap.md:196` から `:198`、`docs/spec/core-ir.md:165`)。この方針なら古い関数は古いまま呼ばれ続けるので、間接参照 (関数のセルを書き換える方式) は要らない。

他の処理系は再定義の見せ方が違う。Julia は、新しい関数の定義を次に最上位から始まる呼び出しから見せ、グローバル変数の更新は直ちに見せる ([world age](https://docs.julialang.org/en/v1/manual/worldage/)、[論文](https://dl.acm.org/doi/pdf/10.1145/3428275)の 1 節と 3.1 節)。これは、動的な呼び出しが最新の定義を引く言語の事情である。eml の方針に近いのは、Chez Scheme の[マニュアルページ](https://www.mankier.com/1/scheme)にある「再定義の前に入力したコードは元の束縛を使い続ける」(`cons` の例) という記述である。GHCi の再定義の扱いは確認できていない。

型を定義し直した場合は、同じ名前でも別の型として扱う必要がある。ヒープの `Data` はタグしか持たないので (`heap.rs:43`)、古い値の表示には古い配置を使う。配置を型の定義の識別子で引く表にすれば足りる (推測)。ロードマップは「定義し直した型の表示」を未決の論点にしている (`docs/future/roadmap.md:207`)。

費用の測定では、入力のたびにプログラム全体を処理し直す方式 (`Session::compile`、`crates/eml_cli/src/lib.rs:162`) は、Prelude が 69 行、Fs が 7 行の今は、`main () = println "hi"` で 2.4 ms、関数 100 個の連鎖で約 5 ms、1000 個で約 27 から 37 ms だった (E2)。標準ライブラリが S4 で大きくなると基底の費用は上がる。ただし今は、増分の lowering を後回しにしても対話の応答性は損なわれない。

#### 2.2 トップレベルの束縛と、セッションをまたぐ状態

- 今の言語のトップレベルの値は、評価結果を覚えない。`x : Int` と `x = 1 + 2` を 2 回使うと、Core IR は引数 0 の関数 `x` への `call x()` を 2 回出す (E6、検証済み)。REPL の `let x = e` は一度だけ評価して覚える必要があるので、ファイルのトップレベルの値とは別の意味になる。
- 残してよいのは `Unr` の値だけで、`Lin` の値は入力の中で使い切る (`docs/future/roadmap.md:192`)。この規則は既存の線形性の検査をそのまま使える。
- IR を変えない実現方法を提案する。入力ごとに、使うセッション値を引数に取る関数 `input$N(g1, .., gk)` を作る。ランタイムはセッションの表から値を `dup` して渡す。引数は所有で渡す規約なので (`docs/spec/core-ir.md` の「消費」)、Perceus の規則に収まる。新しい `Rhs` や `Atom` は要らない (推測、未実装)。
- セッションの表はヒープの根になる。`Heap::live_objects` と同じ数え方で、異常終了の後に「根だけが生きている」ことを検査できる。

#### 2.3 実行時エラーと未処理のエフェクトからの回復

- 実験 E3。深さ 1000 の非末尾再帰の底で `1 / 0` を起こすと、CEK のヒープに Frame 1002 個と String 1000 個が残った。`rt.cont` を `decref` すると、フレームと退避した値は全部解放された。残ったのは誤りを起こした関数の局所変数 (環境 `Env`) の String 1 個である。フレームは「ちょうど所有している参照だけ」を持つ設計 (`docs/spec/core-ir.md` のインタプリタの節) なので継続の連鎖は正確に回復できるが、実行中の関数の環境は回復できない。
- 回復の方法は二つある。
  - 巻き戻し案。呼び出し位置ごとの所有スロットの表 (今の `saved` に相当) と、失敗しうる位置の所有集合を表にして、フレームごとに `decref` する。VM でもネイティブでも各呼び出し位置の表が要る。
  - 走査案。セッションの根から到達できるオブジェクトを印付けし、到達できないものを解放し、到達できるものの RC を「到達できるオブジェクトからの入辺の数 + 根の参照」に数え直す。コードの生成方式に依存せず、正しさが構成から分かる。費用は生存オブジェクトの数に比例するが、実行するのは異常終了のときだけである。`children` (`heap.rs:700`) が既に子の列挙を持つ。
- Perceus の論文も、例外、`longjmp`、再開されない継続のような非局所の制御は、RC の挿入の前提である明示的な制御フローの外にあると述べる (2.7.1 節)。したがって、実行時エラーと割り込みの後の回復は、Perceus の規則だけでは決まらない。
- 走査案を推奨する。ただし RC の数え直しの健全性 (Rust 側の一時的な参照が残っていないこと、不死のオブジェクトの扱い) は、私の設計案であり、文献で直接の先例を確認できていない (推測、未検証)。
- 未処理のエフェクト。今の `find_handler` は連鎖が `Root` に届くと内部エラーにする (`effects.rs:42`)。ロードマップは、REPL の式を row が `<IO>` に収まるものに限る方針で、「`IO` 以外のエフェクトを既定の handler で許すか」を未決としている (`docs/future/roadmap.md:191`、`:205`)。型検査が弾く間は到達しない。許す場合の設計として、evidence passing では、handler の無い操作の yield が最も外側 (今の `Root` に当たる位置) まで届く。その yield は「操作名、引数、継続 (クロージャ)」を持つ値として実行の呼び出し元 (REPL) に戻る。REPL はそれをエラーとして報告し、継続を `drop` して `Lin` の値を破棄するか、ホスト側の既定の handler に渡せる。OCaml 5 は、最も外側まで届いた処理されない effect を、継続を `Unhandled` 例外で再開して後始末を走らせる設計にしている ([Retrofitting Effect Handlers onto OCaml](https://arxiv.org/abs/2104.00250) の 3.2 節、全文を読んだ)。eml の `drop k` はオブジェクトの解放で `Lin` を破棄する (`docs/spec/core-ir.md` のインタプリタの節) ので、再開せずに捨てる方が単純である。

#### 2.4 評価の中断 (Ctrl+C)

Core IR は辺がすべて前向きで、ループの辺が無い (`docs/spec/core-ir.md:57` の R2)。制御が関数をまたぐのは `Machine::transfer` (`machine.rs:255`) だけで、呼び出しも戻りも `bind` と `terminate` から `transfer` を通る (`machine.rs:103`、`:198`、`:204`)。したがって、ここで割り込みの旗を見れば、有限の時間で必ず確認できる。実験 E4 では、この 1 か所の確認で、300 ミリ秒後の割り込みが 0.303 秒で実行を止めた。確認は `AtomicBool` の緩い読み出し 1 回で、E5 では `fib 30` が 1.033 秒から 1.048 秒、1000 万回のループが 2.896 秒から 2.822 秒で、サンドボックスの揺れの範囲だった。長く走る extern (`read_all`、巨大な文字列の連結) は安全点の間にあるが、各 extern の仕事は入力の大きさに比例して有限である。

OCaml のネイティブコードは、[シグナルをヒープの確保のときにだけ検出する](https://ocaml.org/manual/5.0/native.html)。eml は呼び出しごとの確認を選べるので、確保しないループも止められる。Chez Scheme は割り込み文字で[デバッグ用の handler に入る](https://www.scheme.com/csug7/use.html)。GHC のリンカは、割り込みを受けたくない区間を `mask_` で守る (GHC 7.4.1 のソースの[コメント](https://downloads.haskell.org/~ghc/7.4.1/docs/html/libraries/ghc/src/Linker.html)、現行は未確認)。eml の REPL でも、コード表を更新する区間 (変換の結果の確定、リンク) では割り込みを保留するのがよい (推測)。

VM とネイティブでは、確認を呼び出しと戻り (VM) と関数の入口 (ネイティブ) に置く。スタックの深さの上限の確認と同じ位置にまとめられる (推測)。OCaml 5 のファイバは小さいスタックから始めて足りなければ倍に拡張するので、関数の入口にスタックオーバーフローの確認を入れている ([Retrofitting](https://arxiv.org/abs/2104.00250) の 5.2 節)。

資源の上限も要る。E4 で、非末尾の無限再帰は 1 秒で約 582 MB になった。CEK ではメモリが尽きるまで止まらない。VM では、深さの上限か確保量の上限の超過を `Fault` にして、同じ回復の経路に流す。Rust の標準ライブラリにはシグナルを扱う API が無い (推測)。SIGINT の受け方 (外部 crate の選定) は未調査で、受ける側は旗を立てるだけにする。

#### 2.5 値の表示

- 実行時の値は自分の型を持たない。CEK の `Value::Tag(1)` は `Bool` の `True` なのか、別の列挙型のタグなのか分からない (`heap.rs:17`、`:43`)。ネイティブの `tobj` では `Int` の即値と `enum` のタグも区別できない。表示には、入力の式の静的な型 (`TypedProgram`) と配置の表 (`Program.layouts` は型の名前とコンストラクタの名前を持つ、`crates/eml_core_ir/src/lib.rs:66`) が要る。
- ロードマップは、結果を S4 の `Show` で表示する方針にしている (`docs/future/roadmap.md:194`)。`Show` を使えば、表示は通常のコードとして実行され、ランタイムには何も要らない。VM でもネイティブでも同じである。ロードマップはコマンドも GHCi に寄せる方針である (`docs/future/roadmap.md:195`)。
- 残る課題は、`Show` を持たない型 (関数、継続、`Lin` の型) と、折り返しや打ち切りのある整形である。これには、ホスト側の型指向の表示器が要る。ランタイムに「値の検査 API」 (`inspect(word, repr) -> {Int, Tag, Str, Data{tag, fields}, Closure, ...}`) を置き、表示器はそこから静的な型に従って子をたどる。深さと要素数の上限を持たせ、巨大なリストや循環の疑いで止められるようにする。表示中も割り込みの旗を見る。
- 検査 API は、CEK、VM、ネイティブのそれぞれが実装できる。語表現では、`tobj` の位置にあるのが `Int` か `enum` かは静的な型が決めるので、表示器は型を持って降りる (推測)。

#### 2.6 JIT とネイティブコードとの混在

混在で問題になる接続点は五つある。

| 接続点 | 混在で起きること | 設計の方針 |
|---|---|---|
| 関数値 | VM の関数をネイティブが `apply` する。逆も起きる | クロージャは関数番号と引数列を持ち、関数表 `FnEntry` を引く。一様なエントリ (`f$boxed`) で呼ぶ |
| データ | VM が作ったリストをネイティブの `map` が読む | 値の語表現、ヘッダ、記述子を共通にする |
| 継続 | VM の `perform` が、ネイティブのフレームをまたぐ | evidence passing で継続をクロージャにし、yield を戻り値と旗で伝える |
| extern | 両方が同じ表の関数を呼ぶ | `eml_extern` の表と、語 ABI で一度だけ書いた実装を共有する |
| 割り込みと誤り | ネイティブの実行中に Ctrl+C や実行時エラーが起きる | 旗と関数の入口の確認 (割り込み)。誤りの中断は未設計 |

関数番号経由の呼び出しは、今のヒープが既に持つ形である。`Closure.function` は番号であり、ポインタではない。ネイティブのコードが入った関数も番号で呼べば、位置に依存しないまま混在できる。Lean のインタプリタは、シンボルを探して `_boxed` 版か通常版を使う。Zulip に載った利用者の perf の出力では、シンボルの表の検索 (`rb_map::find`) が約 11%、`lookup_symbol` が約 2.7% を占めていた ([Zulip](https://leanprover-community.github.io/archive/stream/270676-lean4/topic/include.20lean.2Eh.html)、1 つの負荷での計測)。eml は添字で引けるので、この費用を避けられる。

#### 2.7 Perceus の RC と multi-shot 継続との関係

- セッションの根は参照を 1 つ持つ。入力の実行は根から `dup` した値を引数に受け取るので、その値の RC は 2 以上になり、一意でない。`++` の左辺が一意なときだけその場で連結する規則 (`heap.rs:307` の `append_str`) は働かず、毎回コピーになる (推測)。セッション変数に `s ++ "x"` を繰り返し足す使い方は、入力ごとに長さに比例する費用がかかる。再束縛の後で古い根を手放す順序を決めておく必要がある。
- 継続のフレームは、関数と再開位置 (`resume: u64`) を持つ (`heap.rs:80`)。ヒープに残る継続は、捕獲した時点のコードの位置を指す。関数表が追記だけであれば、この参照は安定する。evidence passing の後は、継続はクロージャになり、関数番号だけを参照する。
- 共有された継続の再開は、区間のフレームを写す (`heap.rs:433` の `copy_segment`、`:353` の `take_or_copy`)。evidence passing の後は、共有されたクロージャへの `apply` が中身を写し、子の参照を数え直す (`docs/spec/core-ir.md:167` の「共有されたクロージャへの `apply` は、中身を写す」)。費用は継続の深さでなく、捕まえた変数の数に比例する。
- 文字列のリテラルは、実行を始めるときに `Runtime::new` が一括で不死のオブジェクトにする (`runtime.rs:120`)。セッションでは、入力が足したリテラルだけを追記で作る形にする必要がある。
- `debug_heap` は、実行の終わりに生存オブジェクトを数える (`lib.rs:71`)。セッションでは、入力が正常に終わるたびに「生存オブジェクトが根と不死のリテラルだけである」ことを検査できる。不死のリテラルの `dup` と `decref` が打ち消し合って見つからない既知の穴 (`docs/spec/runtime.md` の `debug_heap` の節) は、同じリテラルを何度も評価するセッションでは見つかりにくさが増すので、入力ごとの検査が役に立つ (推測)。

### 3. ネイティブと混在するための条件 (検証済みの事実と資料の主張)

1. **継続を捕獲できる範囲**。CEK の継続は、ヒープのフレームの連結リストである。ネイティブのコードが C のスタックで動くと、その呼び出しの途中で `perform` が起きたときに、継続はネイティブのフレームを含められない。OCaml 5 も、effect が C のフレームをまたいで伝わることを許さない ([Retrofitting](https://arxiv.org/abs/2104.00250) の 3.1 節、全文を読んだ)。したがって、ヒープのフレームを使う VM とネイティブのコードを同じ実行でつなぐには、継続をクロージャにする evidence passing を Core の変換として先に入れる必要がある (推測。ただし論文の制約から導いた)。
2. **値とオブジェクトの表現**。Lean の FFI の文書は、`Nat` と `Int` を `lean_object *` とし、下位ビットが 1 なら即値、そうでなければオブジェクトへのポインタにすると述べる ([Lean のリファレンス](https://lean-lang.org/doc/reference/latest/Run-Time-Code/Foreign-Function-Interface/))。`lean.h` のコメントは、`m_rc` の符号で単一スレッド (正)、複数スレッド (負)、RC なし (0) を区別する ([lean.h](https://github.com/leanprover/lean4/blob/master/src/include/lean/lean.h)、ヘッダを直接読んだ)。`lean_object` は `m_rc`、`m_cs_sz`、`m_other`、`m_tag` の 8 バイトで、`lean_box` は下位ビットを 1 にした即値を作り、クロージャは関数ポインタ `m_fun` を持つ。eml は関数番号を持つので、位置に依存しない。`lean_alloc_small_object` は確保のたびに `lean_inc_heartbeat` を呼び、`lean_internal_panic_rc_overflow` という RC のオーバーフローの検査もある。eml の不死のオブジェクト (`Header.immortal`、`heap.rs:156`) は、Lean の RC なし (0) に当たる。
3. **GHCi の先例**。GHCi のユーザーガイドは、解釈されるコードとコンパイル済みのコードが並んで動き、起動時に `base` のコンパイル済みの写しを読み込むと述べる ([ユーザーガイド](https://downloads.haskell.org/ghc/latest/docs/users_guide/ghci.html))。GHC 7.4.1 のリンカのコメントは、オブジェクトを先に読み込み、オブジェクトはバイトコードに依存できないとする (現行は未確認)。このため、コンパイル済みのコードからインタプリタのコードを呼ぶ場合は、インタプリタのコードを関数値として渡す形になると考えられる (推測)。GHCi が評価した値を見る [ghc-heap-view の紹介記事](https://www.joachim-breitner.de/blog/580-GHCi_integration_for_GHC_HeapView)は、インタプリタが作った遅延評価の計算 (`_bco`) が、コンパイル済みの値と同じヒープに並ぶことを示している。
4. **Lean の先例**。Lean のインタプリタは、ネイティブのコードがある関数を呼ぶときにそちらへ切り替える ([論文](https://lean-lang.org/papers/lean4.pdf)の抜粋)。FFI の文書は、借用の注釈が ABI と実行時の挙動に影響するのは「コンパイルされたとき、または解釈されたとき」だと書く。インタプリタが同じ ABI の規約に従うことを意味する。Lean 4.22 のリリースノートは、余分な `unbox` はネイティブでは問題にならないがインタプリタでは影響すると述べる ([リリースノート](https://lean-lang.org/doc/reference/latest/releases/v4.22.0/))。
5. **OCaml の先例**。ネイティブコードのオブジェクトファイルはバイトコードと混ぜられず、ネイティブのオブジェクトファイルはトップレベルに読み込めない ([マニュアル](https://ocaml.org/manual/5.0/native.html))。ネイティブのトップレベル `ocamlnat` は、バイトコードの機械ではなくネイティブのランタイムとコンパイル機構の上に作られ、入力ごとにコードを作ってプロセスの中で読み込む ([Fischbach と Meurer の論文](https://arxiv.org/pdf/1110.1029)の 4 節と 5 節、全文を読んだ)。実行系を混ぜずに、ランタイムの層を共有する設計である。
6. **Chez Scheme の先例**。REPL の評価は `compile` (機械語を作る) がもともとの設定である。`interpret` は短く終わる式では `compile` より速いことがある ([system](https://www.scheme.com/csug8/system.html))。インタプリタ方式の Petite Chez Scheme の説明は、コンパイル済みのコードとの速度差を 5 倍前後から 10 倍以上とする ([CSUG](https://scheme.com/csug8/use.html))。
7. **Koka の先例**。Koka の対話環境は、入力ごとに型検査、リンク、実行形式の作成を行う ([Koka のブック](https://koka-lang.github.io/koka/doc/book.html)の表示例)。ブックには、`-O2` を指定して同じプログラムを走らせ直すと 4.104 秒から 0.670 秒になる例がある。この REPL は、入力ごとにヒープを持ち越すものではないと私は読む (推測)。eml が目指すセッション内のヒープの持ち越しは、これとは別の設計である。

## 技術的背景と関連研究

| 技法 | 出どころ | eml の現状との関係 | 読んだ範囲 |
|---|---|---|---|
| 継続を呼び出しスタックで表す effect handler | [Sivaramakrishnan ほか, "Retrofitting Effect Handlers onto OCaml", PLDI 2021](https://arxiv.org/abs/2104.00250) | eml の継続はヒープの連結リスト。OCaml は小さいスタックのファイバを使い、継続は一度だけ再開できる。`multi` は扱えない。C のフレームは捕獲できない。最も外側まで届いた effect は例外で再開して後始末する | 全文 |
| generalized evidence passing と yield bubbling | [Xie と Leijen, ICFP 2021 (PACMPL 5, ICFP, Article 71)](https://xnning.github.io/papers/multip.pdf) | `docs/future/evidence-passing.md` が採る方式。yield は各フレームが継続に自分の続きを足して handler まで戻る。毎回の yield 確認は現代のプロセッサで安いと論文は述べる | 要旨と抜粋 |
| IR インタプリタとネイティブのコードの切り替え | [de Moura と Ullrich, "The Lean 4 Theorem Prover and Programming Language", CADE 2021 (システム記述)](https://lean-lang.org/papers/lean4.pdf) | `f$boxed` と一様な関数の規則は、Lean の `_boxed` と対応する。インタプリタが `_boxed` 版と通常版のシンボルを探すことを Zulip のエラー表示が示す | 抜粋 |
| world age | [Belyakova, Chung, Gelinas, Nash, Tate, Vitek, "World Age in Julia: Optimizing Method Dispatch in the Presence of Eval", OOPSLA 2020 (PACMPL 4, Article 207)](https://dl.acm.org/doi/pdf/10.1145/3428275) | 実行中の呼び出しに再定義を見せないことで、eval がある中でも特殊化とインライン化を安全にする。eml は呼び出し先が名前でなく番号なので、古い関数は古い定義を呼び続け、同様の安全性が追記だけの表から得られる (推測) | 前半の本文 (定義、意味、最適化の節) |
| ネイティブのトップレベル | [Fischbach, Meurer, "Towards a native toplevel for the OCaml language", arXiv:1110.1029 (査読を経た発表先は確認できない。本文の計測は OCaml 3.12.1)](https://arxiv.org/pdf/1110.1029) | ネイティブのランタイムの上に、入力ごとにネイティブコードを作って読み込むトップレベルを作る。実行時のリンカがコードを確保し、再配置し、大域のシンボルを登録する。ランタイムへの追加は、大域のシンボルの管理の層と、実行の入口だけである。eml の追記だけの関数表に当たる。バイトコードとの混在はしない | 全文 |
| 効果ハンドラのトレース JIT | [Gaißert ほか, "Tracing Just-in-Time Compilation for Effects and Handlers", OOPSLA 2025 (PACMPL 9, OOPSLA2)](https://dl.acm.org/doi/10.1145/3763085) | バイトコードにスタックのスタック (metastack) を持たせ、不変の連結リストで表す。JIT が確保を除く。evidence passing と別の道で、同じ VM で JIT まで一貫して扱える | 抜粋 |
| 継続の仮想化 | [Ma, Jung, Zhang, "Virtualizing Continuations", PACMPL 2026](https://dl.acm.org/doi/10.1145/3808289) | スタックのコピーが参照を無効にする問題を、スタック領域の仮想アドレスで解く。eml のフレームはヒープの RC なので、この問題は起きない | 要旨 |
| Wasm の継続命令 | [Phipps-Costin ほか, "Continuing WebAssembly with Effect Handlers", PACMPL 2023](https://dl.acm.org/doi/10.1145/3622814) | 継続を作る、中断する、再開する 3 命令を足して、スタック切り替えを実行系に持たせる設計 | 要旨 |

### Perceus と RC の系譜 (論文の全文を読んだ)

eml の RC は、Lean の借用つき RC ([Ullrich と de Moura, "Counting Immutable Beans", IFL 2019](https://arxiv.org/abs/1908.05647)、今回は要旨のみ) を Koka の Perceus ([Reinking, Xie, de Moura, Leijen, "Perceus: Garbage Free Reference Counting with Reuse", PLDI 2021](https://xnning.github.io/papers/perceus.pdf)、DOI 10.1145/3453483.3454032) が精密な所有の形に整理した系譜にある。Perceus の論文は、Lean の仕事の上に作ったと述べ (1 節)、Lean の IR は部分適用ノード `pap` を使い、第一級のラムダを持たないので、アルゴリズムが違いうるとも述べる (5 節)。今回、Perceus の全文を開き、REPL とネイティブの観点で実装と比べた。

| 論文の内容 | eml の実装 | 差と含意 |
|---|---|---|
| 所有を関数へ渡す精密な RC。参照は最後の使用で手放す (2.2 節、定理 4) | 引数は消費で、`dup`、`decref`、`release` を Perceus のパスが挿入する (`docs/spec/core-ir.md` の「消費」) | 規則は一致する。REPL のセッションの根は所有する参照を 1 つ持つので、入力の実行は `dup` した参照を受け取る |
| drop specialization。一意なら子を手放して解放し、そうでなければ `decref` する (2.3 節の図 1c と 1d) | `release_fields` が、一意なら箱を解放して残さないフィールドを手放し、共有なら残すフィールドを `dup` して `decref` する (`heap.rs:385`) | 図 1d の形に当たる。eml は IR の命令 `release` として置くので、VM とネイティブが同じ 1 命令を実装できる |
| 借用の環境 Δ。`dup` を葉まで遅らせる (3.2 節) | `switch` と `unpack` はフィールドを借りて始め、行き先で `dup` か `release` を置く (`docs/spec/core-ir.md`) | 同じ考え方である |
| クロージャの適用。捕獲した変数を `dup` して、クロージャを `drop` する (図 7 の `appr`) | `take_or_copy` が、一意なら中身を取り出し、共有なら写して子を `dup` して元を `decref` する (`heap.rs:353`) | 結果は同じで、一意のときの `dup` と `drop` の対を省いている |
| reuse analysis と reuse specialization (2.4 節、2.5 節) | 未実装 (`docs/implementation/status.md`)。設計は `docs/future/roadmap.md` | REPL のセッション値は RC が 2 以上になるので reuse の速い経路に入らない。論文が `deriv` と `nqueens` で述べた「共有が多いと最適化が効きにくい」状況に近い |
| 明示的な制御フローが前提。例外、`longjmp`、再開されない継続は、RC の挿入の外にある (2.7.1 節) | 実行時エラーと割り込みは、Perceus の規則の外の中断である。CEK ではフレームと環境が残る (E3) | 論文は、C++ の `shared_ptr` がスコープに結び付いて巻き戻しに乗ることを対比として挙げる。Perceus の所有渡しは巻き戻しに乗らない。したがって、異常終了後の回復は RC の規則だけでは決まらず、提案 2 の走査が要る |
| 複数スレッドの RC。共有の印を負の値で表し、遅い経路を 1 回の比較にまとめる。非常に大きいカウントは動かさない範囲に入れる (2.7.2 節)。全部を原子的にすると 5% から 59% 遅くなった (4 節) | `Header.rc` は `u32` で、`dup` は単純な加算である (`heap.rs:268`)。マルチコアの設計は、符号付き `AtomicI32` の共有の印方式を決めている (`docs/future/multicore.md:32`) | 符号の規約は一致する。論文の「動かさない範囲」に当たるものが eml には無い。長く続くセッションで共有された値は、カウントが増え続ける (提案 11) |
| 循環は扱わない。循環を作るのは可変参照だけである (2.7.4 節、定理 2 の注) | 可変参照は未実装。ロードマップは `Ref` を計画する | 提案 2 の走査は、根から到達できない循環も解放できる (推測) |
| Koka は mimalloc の改造版を使う (4 節) | 確保は Rust の `Vec` と `String` | Lean も mimalloc を選べる (`lean.h`)。共通ランタイムの確保器の選択肢になる |

論文の評価は Koka での結果で、Lean の負荷に近いメモリ確保の多いベンチマークである。eml のスクリプトの負荷に移るには、データ構造の更新と共有の割合が近い必要がある。eml の REPL では、セッション値が共有されるので、reuse の効果は論文の `rbtree` よりも `rbtree-ck` に近いと推測するが、測っていない。


### 効果と RC を前提にした他の処理系との比較

| 処理系 | REPL と実行系 | コンパイル済みコードとの混在 | 継続とエフェクト | メモリ管理 | eml への含意 |
|---|---|---|---|---|---|
| OCaml | トップレベルはバイトコード。ネイティブのトップレベル `ocamlnat` は別に作られた | 不可 (マニュアル) | OCaml 5 はスタックのファイバ。一度だけ再開できる。C のフレームは捕獲できない | 追跡型の GC | 混在しないなら、ランタイムの層だけを共有する形が成り立つ。混在したい eml には当てはまらない |
| Lean 4 | IR インタプリタ。ネイティブのコードがあれば切り替える | 可。`lean.h` は、8 バイトのヘッダを持つ `lean_object` と、ネイティブの関数ポインタ `m_fun` を持つクロージャを定める。perf の出力では、インタプリタが同じ `lean_inc_ref` と `lean_dec_ref` の遅い経路を使う。インタプリタのソースは読んでいない | 今回は調べていない | RC。`m_rc == 0` は RC なし | eml の最も近い先例。`_boxed` と一様なエントリの考え方が `f$boxed` と対応する |
| Koka | 対話環境は入力ごとに検査、リンク、実行形式の作成を行う (ブックの表示) | 該当なし (入力ごとにコンパイルする) | evidence passing と yield bubbling。`multi` の再開も扱う (ブックの目次) | Perceus の RC | 効果と RC の組み合わせの先例。ただし REPL はヒープを持ち越さない (推測) |
| GHCi | バイトコードとコンパイル済みのオブジェクトが並ぶ | 可。同じヒープ | 今回は調べていない | 追跡型の GC | 解釈されるコードを、コンパイル済みコードから見て普通の関数値にする形の先例 |
| Julia | JIT。再定義は世代で遅らせて見せる | 該当なし | 今回は調べていない | 追跡型の GC | 再定義と最適化を両立させる意味の先例 |
| Chez Scheme | REPL はもともと `compile`。`interpret` も使える | 該当なし | 第一級の継続 (CSUG の説明) | 追跡型の GC | REPL を最初からコンパイルにする選択肢の先例。割り込みはデバッグ用の handler に入る |

比較した範囲では、`multi` の継続、正確な RC、REPL、コンパイル済みコードとの混在を同時に持つ処理系は確認できなかった (確度は中)。Koka が効果と RC では最も近いが、REPL は入力ごとのコンパイルである。eml の組み合わせは、各要素の先例を組み合わせる形になり、直接の再現ではない。

### 論文と実装の比較

- **OCaml 5 のファイバ** (全文を読んだ)。論文は、ファイバを malloc で確保し、継続を一度だけ再開できるものにし、継続を捕獲するときにフレームを写さない (5.2 節、5.4 節)。eml は `multi` を持つので、再開のたびに区間を写す (`heap.rs:433`)。論文の方式は eml の `multi` に直接は使えない。`never` と `once` だけなら、スタックを写さない方式が使える。設計文書が選ぶ evidence passing では、`multi` の再開もクロージャの適用になる (`docs/future/evidence-passing.md` の `multi` の行)。
- **OCaml 5 の後始末**。論文は、処理されない effect を `Unhandled` 例外で再開し、既存の例外ハンドラの後始末を走らせる (3.2 節)。eml は継続を再開せず、`Lin` をオブジェクトの解放で破棄する。再開しないので、継続の中のコードが走らない。後始末を外から強制しない代わりに、`Lin` が必ずオブジェクトの解放で破棄される前提 (`docs/spec/core-ir.md`) に依存する。
- **Xie と Leijen** (抜粋のみ)。yield の確認は、効果を持つ関数の呼び出しの後に入る。eml の evidence passing は、row が `<>` か `<IO>` だけの関数では変換しない (`docs/future/evidence-passing.md`)。割り込みと実行時エラーの確認を同じ位置に載せるかは、未決である (下の「改善提案」の提案 3)。
- **Lean の IR インタプリタ**。論文はインタプリタが「ネイティブのコードがある関数では切り替える」と述べる (抜粋)。eml では、関数表 `FnEntry` のエントリ (バイトコードかネイティブか) を呼び出しの接続点にすると、同じことができる (提案、未実装)。

## 改善提案

優先順位は、根拠の強さ、影響、後戻りの費用で決めた。成熟度は、確立した実践、査読済みで採用は少ない、プレプリントのみ、私の設計案 (未検証) のいずれかで示す。

### 提案 1 (最優先): REPL のセッションを、先に CEK の上に作る

- 根拠: 2.1 から 2.5 の要件のうち、増分、セッションの根、回復、割り込み、表示は、実行系が CEK でも VM でも同じである。実験 E3 と E4 は、CEK の上で回復と割り込みが短い変更で実現できることを示した。
- 変更内容は次のとおり。
  - `eml_interp` に `Engine` の trait (`run(entry, args) -> Result<Word/Value, Fault>`) を置き、CEK を 1 つ目の実装にする。
  - `Runtime` から寿命 `'p` を外し、関数表、文字列表、配置の表、エフェクトの表を追記だけの `CodeBase` に移す。
  - `Fault::Interrupted` と割り込みの旗を足す。`Machine::transfer` で確認する。
  - 異常終了の後の回復を足す (提案 2)。
  - セッションの根の表と、`input$N` の呼び出しの規約を足す (2.2)。
  - 評価結果の表示は `Show` に任せ、型指向の表示器は後から足す。
- 期待する効果: REPL の意味論を実行系から切り離して確かめられる。後で VM に載せ替えるときの比較の基準になる。VM が遅れても REPL は使える。
- 費用とリスク: 中。`Runtime` の寿命の整理で、`eml_interp` の内部が広く変わる。テストの期待値は変えずに済む見込み (機械的な追随)。
- 成熟度: 確立した実践 (GHCi、Chez Scheme、OCaml のトップレベルはいずれもセッションを持つ)。確度は高い。
- 検証: 入力の列 (トランスクリプト) を再生する golden テストを作り、「入力列の結果は、同じ定義を 1 つのファイルにしたときの結果と一致する」を性質テストにする。異常終了と割り込みのあと、`live_objects` が根だけになることを検査する。

### 提案 2: 異常終了からの回復は、根からの走査と RC の数え直しにする

- 根拠: E3 で、継続の連鎖の `decref` だけでは環境の局所変数が残った。
- 変更: ヒープに「根から到達できるものを残し、他を解放して RC を数え直す」手続きを足す。短期は `decref(cont)` と環境の破棄で足りる。巻き戻し案は、ネイティブの各呼び出し位置の表が要るので採らない。
- 期待する効果: コードの生成方式に依存しない回復。VM とネイティブで同じ手続きを使える。
- 費用とリスク: 小から中。数え直しの健全性の論証が要る。Rust 側が一時的に持つ参照が残っていないことを、呼び出しの規約で保証する必要がある。`slots` の縮小も同時に足す。
- 成熟度: 私の設計案 (未検証)。確度は中。
- 検証: 誤りと割り込みを注入する性質テストで、回復後の `live_objects` と各オブジェクトの RC が「根だけから数え直した値」と一致することを確かめる。`--debug-heap` の漏れ検出を、セッションの終了時にも流す。

### 提案 3: 割り込みと資源の上限を、最初から実行系の規約にする

- 根拠: E4 と E5。割り込みの確認は `transfer` の 1 か所で足り、費用は揺れの範囲だった。無限再帰は 1 秒で 582 MB になる。
- 変更: CEK では `transfer`、VM では呼び出しと戻り、ネイティブでは関数の入口に確認を置く。確認する旗は `Ctx` に持つ。深さの上限とオブジェクト数の上限を同じ位置で見る。コード表の更新中は割り込みを保留する。
- 費用とリスク: 小。VM とネイティブでは、割り込みと evidence passing の yield の確認を 1 つの旗にまとめられるかを設計で決める必要がある。
- 成熟度: 確立した実践 (OCaml 5 は関数の入口でスタックの確認を行う。Lean は確保のたびに `lean_inc_heartbeat` を数え、確保回数による上限の先例になる)。確度は高い。
- 検証: 割り込みを注入する実験 (E4 の方式) を、末尾再帰、非末尾再帰、`handle` を含む再帰、extern を含むループで繰り返し、停止までの時間と回復後のヒープを記録する。

### 提案 4: evidence passing を、VM より先に Core から Core への変換として入れる

- 根拠: OCaml 5 の論文は、effect が C のフレームをまたげないとする。CEK のヒープ上のフレームは、ネイティブのスタックのフレームを含められない。設計文書は evidence passing を採ると既に決めている (`docs/future/evidence-passing.md`)。
- 変更: 前回の提案 5 の優先順位を上げる。変換後の IR を CEK で実行して UI テストと比べる (設計文書の差分テスト)。lowered Core の verifier の段を足す。
- 期待する効果: 継続がクロージャになるので、ヒープの種類から `Frame`、`Continuation`、`Handler` が消える。VM は継続の捕獲を持たず、ネイティブと同じ規約で yield を見る。REPL の未処理のエフェクトは、最も外側まで届いた yield を呼び出し元が受け取る形になる。
- 費用とリスク: 大。yield の確認が全関数に入る。`never` の中断での `Lin` の破棄が未決である (`docs/future/evidence-passing.md:107`)。
- 成熟度: 査読済みで、Koka の C バックエンドが採用している (論文の主張)。eml への採用は未実施。確度は中から高い。
- 検証: `handler_visits` が深さに依存しないこと。UI テストの出力と `RunStats` の RC の回数が、変換の有無で一致すること。

### 提案 5: 共通ランタイム (`eml_rt`) を作り、VM をその上に置く

- 根拠: 2.6 と調査結果の 3。
- 選択肢の比較。

| 案 | 内容 | 利点 | 欠点 |
|---|---|---|---|
| A | VM も現行の安全ヒープ (`Value` enum) の上に置き、ネイティブは別のランタイムにする (設計文書の現状) | `unsafe` が要らない。CEK と実装を共有できる | ネイティブのコードと同じデータを共有できない。混在は、データの写しか、一階のデータだけの受け渡しに限られる (推測) |
| B | 語とポインタのヘッダ付きオブジェクトを共通にし、安全性は検査モードで補う (推奨) | VM とネイティブが同じ値、ヒープ、RC を共有できる。Lean と同じ設計 | `unsafe` の核を持つ。検査モードの作り込みが要る |
| C | 語を共通にするが、ヒープは添字と世代番号のまま (ネイティブ着手時にポインタへ替える) | 安全性を保ったまま語表現へ移れる | 添字の読み出しがネイティブのコードにも入る。符号化が替わるときに VM の高速経路を直す必要がある (推測) |

  案 B を推奨する。確度は中である。決め手は、混在を目標に置く場合に VM の命令の形が値の符号化に依存することである。後から替えると、VM の命令集合と extern の実装の両方が影響を受ける。
- 設計の骨子 (確度は中。符号化の細部は低い)。
  - 値は 64 ビットの語。`obj` と `tobj` の位置では、下位ビットが 0 ならポインタ、1 なら即値 (63 ビットの `Int`、`enum` のタグ、`unit`、関数定数の番号)。`int`、`enum`、`unit` の変数は、タグなしの生のスロットに置く。
  - `box` は、`Int` が 63 ビットに収まれば即値にし、収まらなければ `Boxed` オブジェクトを確保する。`unbox` は逆である。`Int` のオーバーフローの意味 (`IntegerOverflow`) は変わらない。Koka も、63 ビットまでの値型をその場で box し ([ブック](https://koka-lang.github.io/koka/doc/book.html))、Lean も下位ビットが 1 のとき即値にする。
  - ヘッダは 8 バイト程度。RC の符号は Lean に合わせ、0 を RC なし (今の不死のオブジェクト)、正を単一スレッド、負を複数スレッド用に予約する ([lean.h](https://github.com/leanprover/lean4/blob/master/src/include/lean/lean.h))。種類は `String`、`Closure`、`Data`、`Boxed`、`External` (`File` の破棄処理を持つ) とする。`Frame`、`Continuation`、`Handler` は提案 4 の後に要らなくなる。
  - 関数表 `FnEntry` は、引数の数、一様かどうか、実体 (バイトコード、ネイティブ、未生成) を持つ。クロージャは、今と同じく関数番号と引数列を持つ。
  - 実行文脈 `Ctx` は、割り込みの旗、深さと確保量の上限、yield の旗と evidence のベクタ、セッションの根、出力先を持つ。VM もネイティブも `Ctx` へのポインタを先頭の引数に受け取る。ネイティブの関数の ABI 候補は `extern "C" fn(*mut Ctx, Word, ..) -> Word` で、一様なエントリは引数を配列で受ける。extern は `fn(&mut Ctx, &[Word]) -> Result<Word, Fault>` の形で一度だけ書き、ネイティブからは同じ関数を C の ABI の薄い包みで呼ぶ (どちらも案であり、未実装)。
  - 検査モード。解放したオブジェクトを再利用せず、ヘッダに印を付けて、`dup`、`decref`、参照の使用で解放済みを検出する。確保の台帳を持ち、`--debug-heap` の漏れ検出と同じ数え方で `live_objects` を返す。今の世代番号の検査の代わりになる。
  - `unsafe` は小さい核 (確保、解放、ヘッダ、フィールドの読み書き、`dup`、`decref`) に閉じる。設計文書は、ネイティブのランタイムの `unsafe` を Miri で検査する方針を既に持つ (`docs/future/multicore.md:234`)。
- 期待する効果: ネイティブと同じヒープを VM が使える。`Data` が「96 バイトのスロットと別のフィールド配列」から「ヘッダとフィールドが続く 1 ブロック」になり、1 オブジェクトの大きさが数語に縮む (E1、推測)。
- 費用とリスク: 大。`unsafe` を導入する。`runtime.md` の「`unsafe` が要らない」方針を改める必要がある。CEK の `Value` との橋渡しは要らない (CEK は自分のヒープを持ち続ける)。代わりに extern の実装が CEK と共通ランタイムで二重になる。差分テストで食い違いを検出する。S4 で `String` の extern が増える前に、二重の実装を許すか、`Rt` の trait を介して一度だけ書くかを決める。
- 成熟度: Lean と Koka が実用している設計。eml への導入は未実施。確度は中 (符号化の細部は低い)。
- 検証: extern の実装を CEK と共通ランタイムで同じ入力に流して結果を比べる。Miri を通す。UI テストを `--debug-heap` 付きで両方の実行系で流し、出力と `RunStats` の RC の回数を比べる。ベンチマーク (`fib`、`loop`、`state`、リストと木) で CEK との差を記録する。

### 提案 6: 関数表 `FnEntry` を VM とネイティブの接続点にする

- 根拠: Lean のインタプリタは、シンボルを引いてネイティブの実装に切り替える。GHCi は、コンパイル済みコードからインタプリタのコードを関数値として呼ぶ。eml は `f$boxed` で、関数値として参照される関数がすべて一様になる (`docs/spec/core-ir.md:165`)。
- 変更: VM からネイティブの関数を呼ぶときは `f$boxed` の一様なエントリを使い、引数の `box` と結果の `unbox` を VM 側に持つ。ネイティブからバイトコードの関数を呼ぶときは、ランタイムの `apply` を呼ぶ。直接呼び出し (非一様な `f`) は、同じ翻訳単位の中だけにする。
- 期待する効果: 標準ライブラリを一度だけネイティブにして、ユーザーのコードをバイトコードで動かせる。再定義にも強い (T3 は呼ぶ関数の `ret` だけで決まる)。
- 費用とリスク: 中。一様なエントリの `box` 費用が VM 側に残る。Lean 4.22 のリリースノートが示すとおり、余分な `unbox` はインタプリタでは無視できない。
- 成熟度: Lean と GHCi で実績がある。eml では未実施。確度は中。
- 検証: 同じ UI テストを「全部バイトコード」「標準ライブラリだけネイティブ」「全部ネイティブ」で流して出力を比べる。

### 提案 7: 値の表示は `Show` を第一とし、型指向の表示器を足す

- 根拠: ロードマップは `Show` での表示を決めている。実行時の値が型を持たないことは検証済み (2.5)。
- 変更: 第一に `Show` を使う。`Show` を持たない型 (関数、継続) は固定の文字列で表示する。整形 (折り返し、打ち切り) が要るときに、`inspect` API の上に型指向の表示器を作る。表示中も割り込みを見る。
- 費用とリスク: 小。`Show` の導出は S4 に依存する。
- 成熟度: 確立した実践。確度は `Show` の部分が高く、型指向の表示器は中。
- 検証: 入力の型ごとの表示のトランスクリプトテスト。巨大なリストと深い入れ子で打ち切りと割り込みを確かめる。

### 提案 8: 再定義の意味と `:load` の扱いを決める (設計の判断)

- 根拠: ロードマップは「後の入力で定義し直すと、変わるのはそれより後の入力だけ」としている。Chez Scheme の記述もこれに近い。Julia の意味は違い、GHCi は未確認である。確度は中。
- 変更: 関数表は追記だけにする。再定義は新しい番号の関数と、名前から最新の番号への写像で表す。型を定義し直した場合は、別の型の識別子を作る。`:load` で型が変わるときは、その型に依存するセッション値を破棄する案が単純である (推測。GHCi の挙動は未確認)。
- 検証: 再定義と型の再定義のトランスクリプトテスト。古いクロージャが古い定義を呼び続けること、古い値が古い配置で表示されることを確かめる。

### 提案 9 (後回しにできる): 増分の lowering

- 根拠: E2 で、全体を処理し直す費用は小さい。S4 で標準ライブラリが大きくなると基底の費用が上がる。
- 変更: 定義ごとの Core IR を保存する (ロードマップの決定どおり)。配置の番号を正規の順にする。`f$boxed` は追加だけにする。
- 成熟度: 設計は文書にある。未実施。確度は中。
- 検証: 基底の費用 (`main () = ()`) と、入力 1 つあたりの追加費用を測り、全体処理との差を比べる。

### 提案 10 (投機的): JIT と階層化

- バイトコードからの copy-and-patch ([Xu と Kjolstad, OOPSLA 2021](https://arxiv.org/abs/2011.13127)、DOI 10.1145/3485513、要旨のみ)、トレース JIT ([Gaißert ほか](https://dl.acm.org/doi/10.1145/3763085))、ネイティブへの階層化は、提案 5 と 6 が揃ってから検討する。トレース JIT は、バイトコードにスタックのスタックを持つ別の道で、evidence passing とは前提が合わない。確度は低い。成熟度は、eml の前提との適合を含めて未確認である。

### 提案 11: RC の上限の扱いを決める

- 根拠: Perceus の論文は、非常に大きいカウントを動かさない範囲に入れてオーバーフローを避ける (2.7.2 節)。Lean は RC のオーバーフローを検査する (`lean_internal_panic_rc_overflow`)。eml の `Header.rc` は `u32` で、`dup` は検査のない加算である (`heap.rs:268`)。
- 変更: セッションの根が持つ値は、長いセッションで共有され続ける。上限を超えたカウントは動かさない (不死にする) 規約を、共通ランタイムのヘッダに入れる。不死の規約は既にある (`Header.immortal`)。
- 費用とリスク: 小。上限に達した値は解放されなくなる。
- 成熟度: Koka と Lean が採用している。確度は中 (上限の値は論文の 2 の 30 乗が一例)。
- 検証: `dup` を上限まで繰り返す単体テストで、値が解放されず、RC の釣り合いの検査が上限の値を除くことを確かめる。

### 現在の設計で維持してよい点

- 検査付きヒープと CEK を参照実装として残すこと。独立した 2 つの実装を比べられる。
- ループの辺が無い Core IR (R2)。割り込みの確認が呼び出しと戻りだけで済むのは、この性質による。
- `Closure.function` を番号にしていること。位置に依存せず、混在と再定義に強い。
- `f$boxed` と T3 による ABI の追加だけの変更。REPL で古い定義を書き換えずに済む。
- `RunStats` の「時間でなく回数」の方針。VM と共通ランタイムの差分テストにそのまま使える。

### 移行の手順と各段階の検証

| 順 | 変えること | 検証 |
|---|---|---|
| 0 | 計測の基準を作る。`fib`、`loop`、`state`、リスト、木のベンチマークと、トランスクリプトのテストの枠を作る。`Engine` の trait を置く。前回の提案 1 (CEK の確保の除去) は、基準線のためにここで入れてよい | UI テスト全部の出力が変わらない。各ベンチマークの実行命令数と時間を記録する |
| 1 | CEK に REPL のセッションを載せる (提案 1、2、3、7 の第一段、8)。`Runtime` を所有型にし、`CodeBase` を追記だけにする | トランスクリプトの golden テスト。誤りと割り込みの後の `live_objects` |
| 2 | evidence passing と lowered Core の verifier (提案 4) | 変換の有無で UI テストと `RunStats` の RC の回数が一致する。`handler_visits` が深さに依存しない |
| 3 | 共通ランタイム `eml_rt` (提案 5)。extern を語 ABI で書く。docs の spec を更新する | extern の差分テスト、Miri、`--debug-heap` の互換検査 |
| 4 | バイトコード VM (lowered Core から直接、レジスタ型) を `eml_rt` の上に作る。安全点と深さの上限を入れる | 全 UI テストとトランスクリプトを CEK と比べる。ベンチマークで CEK との差を記録する。割り込みの注入実験を VM で繰り返す |
| 5 | ネイティブ (Cranelift) と `FnEntry` (提案 6) | 3 通りの混在 (全部バイトコード、標準ライブラリだけネイティブ、全部ネイティブ) で出力を比べる |
| 6 | 増分 lowering (提案 9)、JIT の検討 (提案 10) | 入力 1 つあたりの追加費用の測定。JIT は試作で速度と起動の遅延を測る |

VM をどうしても早く欲しい場合の代案がある。手順 2 と 3 を飛ばして、現行の `Runtime` (ヒープ上のフレーム) を再利用する VM を作る。その VM は REPL だけに使え、ネイティブとは混在できない使い捨てになる。前回の「第 1 段」に当たる。この場合も、手順 1 は先に済ませておく必要がある。

## 確度と未確認事項

- **コードに関する事実**。行番号、構造、実験の結果は、読んで確かめたか実行した。実験は 1 台のサンドボックスで、release ビルド、各 3 回の中央値である。時間の揺れは大きい (`loop` で 2.8 から 2.9 秒)。割り込みの確認の費用は「揺れより小さい」までしか言えない。
- **根拠の強さ**。結論のうち、確度が高いのは次の二つである。REPL の要件は現行ランタイムの改修で満たせること (E3 と E4 の実験と、`Runtime<'p>` の寿命の読み取り)。CEK は参照実装として残すこと。確度が中から高いのは、混在には共通の値表現が必要だという点である。これは Lean、GHCi、OCaml の事例と、OCaml 5 の論文の制約から導いたもので、eml で混在を試したわけではない。
- **推測として書いた点**。`Data` のフィールド配列が別の確保であることの費用、`Slot` の縮小の見込み、トップレベル以外の値の再評価、Rust の標準ライブラリにシグナルの API が無いこと、`:load` で型が変わるときの扱い、`input$N` の方式、走査案の数え直しの健全性、GHCi が同じオブジェクト表現を使うこと、Koka の REPL がヒープを持ち越さないこと。
- **読んだ範囲**。Retrofitting Effect Handlers onto OCaml は全文を読んだ。全文を読んだのは、Retrofitting Effect Handlers onto OCaml、Perceus、Fischbach と Meurer の論文である。Belyakova ほかは前半の本文を読んだ。Xie と Leijen、Lean 4 の論文、Gaißert ほか、Ma ほか、Phipps-Costin ほか、Ullrich と de Moura、Xu と Kjolstad は、検索結果の抜粋か要旨だけを読み、本文の主張は使っていない。GHC のリンカのコメントは 7.4.1 のものである。現行の GHC の挙動は未確認である。
- **依存の版**。`Cargo.lock` の主な依存は `logos` 0.16.1、`rowan` 0.16.1、`la-arena` 0.3.1、`ariadne` 0.6.0、`clap` 4.6.7、`insta` 1.49.0 で、`eml_runtime` は外部の依存を持たず、`eml_interp` は `eml_extern`、`eml_core_ir`、`eml_runtime` だけに依存する (`Cargo.toml`)。今回の問いに関わる版は、ランタイムの設計の参照先 (Koka v3.2.8 のブック、GHC 9.14.1 のユーザーガイド、OCaml のマニュアル 5.0 と 5.4、Lean の 4.22.0 のリリースノートと `master` の `lean.h`) である。これらは 2026 年 10 月時点で開いた版で、eml の依存の版とは無関係である。フロントエンドの crate の既知の問題は、今回の問いに関わらないので調べていない。実験の `rustc` は 1.97.0 である。
- **行番号の検証**。本文の `path:行` の引用は、抽出して、該当行の内容をスクリプトで確かめた (対象は本文中のすべての完全なパスと、`heap.rs` などの短い名前の引用)。実験で動かしたのはコピーで、引用の行はマウントされたコミットのものである。
- **未設計**。ネイティブのコードで起きた実行時エラー (オーバーフロー、ゼロ除算) を、評価全体の中断へどう変えるか。旗を立てて戻る方式は、効果を持たない関数にも確認を足す。巻き戻しは、Cranelift のコードに展開情報を持たせる必要がある。どちらも未検討である。VM では `Err` を返せば済む。
- **未調査**。Cranelift の API (スタックマップ、フレームの解放、トラップ) の現行の仕様、割り込みを受ける外部 crate、mimalloc などの確保器の選定、複数スレッドの RC の移行費用、`Float` を入れたときの語表現。
- **文書との関係**。この報告の提案 5 は、`docs/spec/runtime.md:5` と `docs/future/multicore.md:34` の記述を変える。`CLAUDE.md` は docs を正とするので、実装の前に spec の更新が要る。
- **ベンチマークの注意**。論文の数値 (たとえば、ハンドラを設けるだけの micro benchmark で通常の再帰の約 10 倍) は、eml の負荷にそのまま当てはまらない。当てはまるには、eml のプログラムが同じ種類の handler の使い方をする必要がある。

## 参考文献

### Web の資料

- eml のリポジトリ内: `docs/spec/core-ir.md`、`docs/spec/runtime.md`、`docs/future/roadmap.md`、`docs/future/evidence-passing.md`、`docs/future/multicore.md`、`docs/implementation/architecture.md`、`CLAUDE.md`
- [GHCi (GHC 9.14.1 User's Guide)](https://downloads.haskell.org/ghc/latest/docs/users_guide/ghci.html)
- [GHC 7.4.1 の `Linker.lhs`](https://downloads.haskell.org/~ghc/7.4.1/docs/html/libraries/ghc/src/Linker.html)
- [OCaml マニュアル: ocamlopt](https://ocaml.org/manual/5.0/native.html)
- [OCaml マニュアル: toplevel](https://ocaml.org/manual/5.4/toplevel.html)
- [Lean リファレンス: FFI](https://lean-lang.org/doc/reference/latest/Run-Time-Code/Foreign-Function-Interface/)
- [Lean の `lean.h`](https://github.com/leanprover/lean4/blob/master/src/include/lean/lean.h) (前半を直接読んだ)
- [Lean 4.22.0 のリリースノート](https://lean-lang.org/doc/reference/latest/releases/v4.22.0/)
- [Lean Zulip: インタプリタのシンボル検索とプロファイル](https://leanprover-community.github.io/archive/stream/270676-lean4/topic/include.20lean.2Eh.html)
- [Chez Scheme User's Guide 第 8 版: Using Chez Scheme](https://scheme.com/csug8/use.html)、[System Operations](https://www.scheme.com/csug8/system.html)、[第 7 版 Using Chez Scheme](https://www.scheme.com/csug7/use.html)、[scheme の man ページ](https://www.mankier.com/1/scheme)
- [Joachim Breitner: GHCi integration for GHC.HeapView](https://www.joachim-breitner.de/blog/580-GHCi_integration_for_GHC_HeapView)
- [Julia マニュアル: The World Age mechanism](https://docs.julialang.org/en/v1/manual/worldage/)
- [The Koka Programming Language (ブック)](https://koka-lang.github.io/koka/doc/book.html)

### 論文

- KC Sivaramakrishnan, Stephen Dolan, Leo White, Tom Kelly, Sadiq Jaffer, Anil Madhavapeddy. "Retrofitting Effect Handlers onto OCaml". PLDI 2021. [arXiv:2104.00250](https://arxiv.org/abs/2104.00250)、DOI 10.1145/3453483.3454039 (全文を読んだ)
- Ningning Xie, Daan Leijen. "Generalized Evidence Passing for Effect Handlers: Efficient Compilation of Effect Handlers to C". PACMPL 5, ICFP, Article 71, 2021. [論文](https://xnning.github.io/papers/multip.pdf)、[Microsoft Research の技術報告](https://www.microsoft.com/en-us/research/wp-content/uploads/2021/03/multip-tr-v2.pdf) (抜粋のみ)
- Leonardo de Moura, Sebastian Ullrich. "The Lean 4 Theorem Prover and Programming Language". CADE 2021 (システム記述). [PDF](https://lean-lang.org/papers/lean4.pdf)、[Springer](https://link.springer.com/chapter/10.1007/978-3-030-79876-5_37) (抜粋のみ)
- Julia Belyakova, Benjamin Chung, Jack Gelinas, Jameson Nash, Ross Tate, Jan Vitek. "World Age in Julia: Optimizing Method Dispatch in the Presence of Eval". PACMPL 4, OOPSLA, Article 207, 2020. [ACM](https://dl.acm.org/doi/pdf/10.1145/3428275)、[arXiv:2010.07516](https://arxiv.org/pdf/2010.07516) (前半の本文を読んだ)
- Marcell Fischbach, Benedikt Meurer. "Towards a native toplevel for the OCaml language". [arXiv:1110.1029](https://arxiv.org/pdf/1110.1029) (全文を読んだ。論文に発表先の記載は無く、本文の計測は OCaml 3.12.1 と 2011 年の環境)
- Marcial Gaißert, CF Bolz-Tereick, Jonathan Immanuel Brachthäuser. "Tracing Just-in-Time Compilation for Effects and Handlers". PACMPL 9, OOPSLA2, 2025. [ACM](https://dl.acm.org/doi/10.1145/3763085) (抜粋のみ)
- Alex Reinking, Ningning Xie, Leonardo de Moura, Daan Leijen. "Perceus: Garbage Free Reference Counting with Reuse". PLDI 2021. [論文](https://xnning.github.io/papers/perceus.pdf)、DOI 10.1145/3453483.3454032 (全文を読んだ)
- Sebastian Ullrich, Leonardo de Moura. "Counting Immutable Beans: Reference Counting Optimized for Purely Functional Programming". IFL 2019 (arXiv v3, 2020). [arXiv:1908.05647](https://arxiv.org/abs/1908.05647) (要旨のみ)
- Haoran Xu, Fredrik Kjolstad. "Copy-and-Patch Compilation: A fast compilation algorithm for high-level languages and bytecode". OOPSLA 2021, DOI 10.1145/3485513. [arXiv:2011.13127](https://arxiv.org/abs/2011.13127) (要旨のみ)
- Cong Ma, Jonghyun Jung, Yizhou Zhang. "Virtualizing Continuations". PACMPL, 2026. [ACM](https://dl.acm.org/doi/10.1145/3808289) (要旨のみ)
- Luna Phipps-Costin, Andreas Rossberg, Arjun Guha, Daan Leijen, Daniel Hillerström, KC Sivaramakrishnan, Matija Pretnar, Sam Lindley. "Continuing WebAssembly with Effect Handlers". PACMPL, 2023. [ACM](https://dl.acm.org/doi/10.1145/3622814) (要旨のみ)

### 補足ファイル

- `experiments/results.md`: 実験の記録 (E1 から E6)
- `experiments/probe.diff`: コピー上の計測用の変更 (サイズの確認、異常終了後のヒープの表示、割り込みの旗)
- `experiments/*.em`、`experiments/dump_example.rs`: 実験に使ったプログラム
