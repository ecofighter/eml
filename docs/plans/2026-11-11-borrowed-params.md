# 借用パラメータと、`blocking`のCの呼び出しへの大きい引数の貸し出しの段 (borrowed-params)

- 作成日: 2026-11-11
- 基準: main @ f918f5fb (この文書の「背景」と、`ファイル:行`の参照はこのコミットの状態を指す)
- 状態: 計画は確定。10の未解決の項目は、どれも既定の扱いを決めてある

## 1. 目標と成功基準

- 目標: LIR (ロードマップ`borrowed-params`)に借用パラメータを追加し、オブジェクトを読み取るだけの呼び出し先がRCなしで受け取り、呼び出し側が呼び出しごとに`dup`しなくて済むようにする。`jit-core`が構築する借用ABIを準備する。同段において、`blocking` C呼び出しの大きな配列、`String`、`Bytes`引数のコピーを停止し、メモリ安全性を維持したまま (ユーザーの要望により)ページの留め (page pin)の下でプールスレッドに貸渡しする。
- 成功基準: T-1からT-13がそれぞれ1つのコミットとして完了し、各タスクの完了条件を満たし、セクション9の全体的な検証がパスすること。特に、`bench::words_loop`で`rc_increments: 0`となり、どのベンチプログラムの`rc_increments`も増加せず、`reuses`も変化せず (T-9のコマンドC2およびC3)、`blocking`呼び出しが大きな配列のアドレス自体を受け取ること (T-8)。

## 2. スコープ

- スコープ内: LIRにおける借用関数パラメータおよび借用merge-blockパラメータ、Borrowパス (推論、`<f>$owned`コピー、末尾呼び出しの降格および付け替え)、Perceusおよびverifierの変更、巻き戻しエントリ行、新しいベンチプログラム、`eml_rt`のページの留め (T17)、`Op::FfiSubmit`での大きな引数の貸渡し、ドキュメントの修正 (`blocking`が貸渡しを行うかどうかに関する`docs/future/ffi.md`および`docs/future/multicore.md`の既存の矛盾を含む)
- スコープ外:
  - 同一ブロック内の別のパラメータによって貸渡しされるブロックパラメータ (ownerフィールド)。クローズ時にロードマップ「その後の項目」へ移行 (T-13)
  - 軽量な`$owned`ラッパー (コピー形式のみ構築)。クローズ時に「その後の項目」へ移行
  - オブジェクト引数を消費する86個の`Effectful`行 (`println`、`FileIn.open`、`Env`行など)を含む、標準extern行の`borrowed`マスクの変更。後続の行に関するルールはT-3でlir.mdに記述 (D-6)
  - 小さな (最大128ワードの)`blocking`引数の貸渡し。これらはコピーされ続ける (D-7)
  - ネイティブコード生成 (`jit-core`)

## 3. 背景

- 借用パラメータや借用ブロック引数は存在しない。`LirFn`のパラメータは所有権フィールドを持たないエントリブロックのパラメータであり (`crates/eml_lir/src/lib.rs:94-123`)、`Call`は所有権を持たない。verifierの`define()`はすべてのRCパラメータを1としてカウントする (`crates/eml_lir/src/verify.rs:1098-1110`)。Perceusは、エントリのブロックとすべての合流のブロックを、すべてのRCのパラメータを所有した状態で始める (`crates/eml_lir/src/perceus.rs:52-61`)。
- 現在の借用変数は、`switch`のケースフィールド、`unpack`フィールド、およびstruct-`unbox`のリーフのみであり、所有者によって追跡される (`verify.rs:1114-1138`、メッセージ「only borrowed from」)。`extern`および`prim`行は`FunctionRow::borrowed` / `PrimRow::borrowed`を通じて貸渡しを行う。`Rhs::for_each_consumed` / `for_each_lent` (`lib.rs:446-475`) が、読み取りと消費の唯一の定義である。
- `tail`/`return`の前で、Perceusは引数を所有にし、保持しているすべての変数を`decref`する (`perceus.rs:380-435`)。そのため、現在は`TailCall`による降格は発生しない。
- 現在の`wc`内部ループにはバイト単位の`dup`は存在しない (文字列は`tail call`を通じて移動する。ベンチの`byte_loop`は`rc_increments: 0`を留めている、`crates/eml_vm/tests/bench.rs:129-155`)。docs/future/native.mdの「展開と最適化の前提」およびdocs/implementation/benchmarks.md:844にある3.7-4.0xという数値は、baea5793におけるLIRのリポジトリ外CLIFプロトタイプに由来する。呼び出しごとに所有値がインラインではない呼び出し先に渡され、再び使用された場合に呼び出しごとの`dup`が発生する。`bench/words_loop.em` (T-1) では現在`rc_increments: 60000`となっている。
- 同期的な`extern "C"`呼び出しは、すでにヒープポインタを貸渡ししている (`crates/eml_vm/src/ffi.rs:100-130`、docs/spec/object-model.md T16)。`blocking`呼び出しは、`Op::FfiSubmit`ですべての配列/`String`/`Bytes`引数をコピーし、`Op::FfiTake`で`MutUnboxedArray`のコピーを書き戻す (`ffi.rs:168-416`)。これは、1秒の静止 (quiesce)予算が経過すると、まだ実行中のジョブは放棄され、Cが動作し続けている間にヒープが回収または破棄されるためである (docs/spec/vm.md「時計とdriver」、テスト`ffi_abandon.rs:267`)。コピーは無制限であり、`RunStats`ではカウントされない。
- ページが`std::alloc::dealloc`に到達するのは、`dealloc_page` (`release_large`によって呼び出される)および`impl Drop for Pages` (`crates/eml_rt/src/alloc.rs:3290-3410`)のみである。大きなオブジェクト (128ワード超)は自身にページを持つ。オブジェクトワードの下位16ビットをマスクすると、そのページの先頭が得られる (`alloc.rs:792, 861`のVerusの補題)。
- `crates/eml_vm/tests/miri/blocking.lir`はパイプラインの最終LIRの逐語的なコピーである。`crates/eml_vm/tests/miri.rs:640-648`は通常の実行における等価性をアサートしている。そのMiriコストはすでにテストあたりの制限を超えている (docs/future/roadmap.md:237)。
- `docs/future/ffi.md` (89-91行目付近)および`docs/future/multicore.md:84`では、`blocking`パスを「貸渡し」として記述しているが、`ffi.md` (94行目付近)、docs/spec/vm.md「`blocking`の呼び出し」、T16とコードは写すと書き、実際に写している。
- それぞれ敵対的にレビューされ修正された詳細な設計は、以下の付録 (英語)にある: 
  - `docs/plans/2026-11-11-borrowed-params/01-borrowed-params.md`: 借用パラメータ。その「5.7 Units」の1-11は、ここではT-1からT-6およびT-9からT-13に対応している。
  - `docs/plans/2026-11-11-borrowed-params/02-ffi-lending.md`: 大きな`blocking`引数の貸渡し。その単位1-2はT-7およびT-8に対応している。
- 付録の内容が本ドキュメントと矛盾する場合の優先順位: この文書の5と6を優先し、その次に付録に従う。付録内の識別子 (B0-B4, A1-A3, P1-P5, S1-S4, O1-O4, R1, R2, G-n, G-Fn)はプラン内部の名称のみであり、`docs/`やコードに記述してはならない (CLAUDE.md: コードおよび`docs/`で`docs/plans/`を引用することはない)。

## 4. 前提条件

| 要るもの | 使う所 | 確かめ方 | ないとき |
|---|---|---|---|
| Nix devShell (cargo, cargo-nextest, cargo-insta) | すべてのタスク | `cargo nextest --version` → バージョンを表示 | 停止して確認 |
| Verusおよびverusfmt (devShell, x86_64-linuxまたはaarch64-darwin) | T-7 | `cargo verus --version` → バージョンを表示 | 停止して確認 |
| Miri shell `.#miri` | T-7, T-8, T-9, T-12 | `nix develop .#miri -c cargo miri --version` → バージョンを表示 | 停止して確認 |
| PLaMo翻訳 (`plamo-translate` MCPサーバーまたはCLI) | すべての日本語テキスト (CLAUDE.md) | MCPツール`translate`がリストされているか、または`plamo-translate --help`が0で終了すること | CLAUDE.mdの指示に従い、`natural-japanese:natural-japanese`スキルに従う |
| `.#miri`経由のAddressSanitizer (macOSホスト: `aarch64-apple-darwin`) | T-8 | T-8のコマンドが実行可能 | コミットメッセージに「ASan not run locally」と記録。CIはLinuxをカバー |

## 5. 決定事項

### 5.1長期ガイドライン (変更には再検討が必要)

実装がこれらのいずれかと競合する場合は、停止して報告すること。それぞれに適合の確かめを列挙しており、完全な基準テキストは名称付きの付録セクションに記載されている。

#### D-1: 借用性の表現方法

- 決定: `eml_lir::Block`に`borrowed: Vec<VarId>`を追加。`params`の部分列であり、デフォルトは空とする。関数のシグネチャは、エントリブロックの`borrowed`となる。`Call`、`Term::Jump`、`Atom`、`VarInfo`は所有権を持たない。唯一の貸す側は関数のエントリ (呼び出し側)であるため、ownerフィールドは存在しない。マージブロックのパラメータは借用される可能性がある (入力されるすべてのエッジが、エントリから借用した値を渡す)。テキスト形式: `fn`パラメータリストおよびブロックラベルのバインダー前に`borrowed`を記述 (`fn skip(borrowed s.0: obj, i.1: int) -> int {`, `b4(borrowed u.7: obj):`)。LIRの読み手だけが受け付け、Core IRのテキストでは拒否される。
- 理由: 単一の信頼できる情報源 (one source of truth)であること。`VarInfo`はCore IRと共有されている。ロードマップの「owner is a same-block param」という形式にはプロデューサーが存在しない。なぜなら、オーナーを解放するにはエッジごとに異なるフィールドごとの (ctor, slot) が必要になるためである。
- 採らない案: 呼び出し/ジャンプ引数への所有権 (2つの信頼できる情報源)、`VarInfo`内のフラグ、ownerフィールドを持つ同一ブロック内の貸す側 (プロデューサーなし、保留)。
- 適合の確かめ: 付録01 §8 G-1, G-2, G-12 (`borrowed`は`Block`および行ルックアップにのみ出現。RC `unr`パラメータのみ。借用パラメータへのすべてのジャンプは、エントリで借用された変数または非変数アトムを渡す)。
- リポジトリに残す所: docs/spec/lir.md「ブロックの列」 (T-2) および「verifier」 (T-3)

#### D-2: 借用ABIと借用が発生し得る箇所

- 決定: 借用パラメータは、参照を持たない単なるワードである。呼び出し側は、呼び出し先の末尾呼び出しを含め、呼び出しが戻るまでオブジェクトを未変更のまま保持する。呼び出し先はそれを読み取り、さらに貸すことができる。消費用途の前には必ず`dup`を行い、`decref`、`release`、トークン化、またはその場での更新を行ってはならない。借用位置は、直接呼び出しのパラメータ、ブロック引数、およびextern/prim行にのみ存在する。`con`、クロージャキャプチャ、`apply`/`clause`/`par`引数、および呼び出し先、`return`、`box`、`drop`、非借用行の引数は消費する。留められた関数 (`split`または`clause`セット、`$uniform`、`$owned`、EPヘルパー、`RuntimeEntry`名)には借用パラメータはなく、値参照 (`Atom::Fn`、`MakeClosure`)も借用パラメータを持つ関数をターゲットにしない。
- 理由: ネイティブコードは、アクティベーション全体にわたって借用された`String`/`Bytes`のコンテンツポインタと長さをホイストする場合がある。`apply`、clauseエントリ、およびランタイムは所有ワードを渡す。
- 適合の確かめ: 付録01 §8 G-3, G-4, G-5
- リポジトリに残す所: docs/spec/lir.md「関数のABI」 (T-6), docs/future/native.md「呼び出し規約とエントリ」 (T-9)

#### D-3: 名前ルールと名前による適応

- 決定: 関数の印は、直接呼び出しで到達可能なコードに対する推論ルールの最小の固定点である。また、そのコードは、値または呼び出しとして参照する関数の印に依存する場合がある。推論は、呼び出し元、関数自体の値参照箇所、またはどのコピーが存在するかを読み取ることはない。`<f>$owned`は、すべての印がクリアされた後の`f`の完全なコピーである。借用パラメータを持つ関数への値参照は`<f>$owned`に向く。
- 理由: 「同一名称、同一コード」 (docs/spec/vm.md「追記」, REPL)。ロードマップの「ラッパー」はコピーであり、`apply`の末尾サイクルを通じてスタックを一定に保つためである。
- 採らない案: 使用法がABIを決定する (ABIはプログラムの他の部分に依存する)。薄いラッパーのみ (`apply`の末尾サイクルの下にフレームを追加する。最適化として保留)。
- 適合の確かめ: 付録01 §8 G-7; コピーに対するデバッグ用のリビルド・比較アサーション; REPLのトランスクリプトは`Mismatch`チェックでサイレントとなる。
- リポジトリに残す所: docs/spec/lir.md「借用の推論」および「関数のABI」 (T-6)

#### D-4: 末尾呼び出し

- 決定: 借用後、借用位置にあるすべての`tail call`引数は、エントリ借用または非変数アトムとなる。値を自身のフレームで保持して貸す末尾呼び出しは、`!open(g)`時に`let r = call g(..)` / `return r`へとその場で降格される (`g`から末尾到達可能な要素に`tail apply`や`tail clause`が存在しない場合)。それ以外の場合は、`tail call g$owned(..)`へと付け替えられる。オリジナルでは、末尾SCC内のエッジでこれが必要になることはない (推論により渡されたパラメータが所有され、Leanの`ownParamsUsingArgs`が機能するため)。`$owned`コピーにおいても同様のルールが適用される。Perceusは終端子を編集しない。
- 理由: `apply`を経由するものを含む末尾サイクルは、スタックを増大させてはならない。呼び出しチェーンごとのフレームの降格数は、関数数によって制限される (付録01 §2「フレームの上限」)。
- 適合の確かめ: 付録01 §8 G-10; T-11のスケーリングテスト (a)
- リポジトリに残す所: docs/spec/lir.md「末尾の位置の呼び出し」 (T-9); lir.md:64および :608にある、Perceusに対して降格を留保している記述を削除する。

#### D-5: メカニズムはポリシーに依存しない

- 決定: Perceusおよびverifierは、推論によるものだけでなく、静的ルールS1-S4 (付録01 §5.4)を満たすあらゆるマーキングに対して正しい。verifierは命令ごとに、まずすべての消費を当て、その後に貸した各変数が有効であることを要求する。Perceusは、同じ命令が持ち主を最後に消費する、貸した変数を`dup`する。
- 適合の確かめ: 付録01 §8 G-11, G-13
- リポジトリに残す所: docs/spec/lir.md「verifier」 (T-3), 「Perceus」 (T-4)

#### D-6: 待ちをまたぐ値は数えた参照で持ち、externの行は変えない

- 決定: yieldの地点をまたいで生存するすべてのRC値は、継続クロージャによって独自のカウント付き参照を通じて保持される (キャプチャは消費位置であり、継続関数は留められる)。推論による印の下では、エントリ借用された値は決してキャプチャされない。`blocking`呼び出しの場合、これは以下を意味する: `FfiSubmit`から`FfiTake`が戻るまで、各オブジェクト引数は、呼び出し元タスクのフレームまたは継続のみが解放できる参照を通じて少なくとも1のカウントを持ち、`MutUnboxedArray`引数はちょうど1つの参照を持つ。この段ではyieldの地点を追加、削除、または移動せず、`submit`/`io_wait`/`take`のローアリングも変更せず、またC行が`obj`引数を借用することも変更しない。標準行の`borrowed`マスクは変更されない。後続の行に関するルール: 行が引数を借用するのは、それを読み取る場合のみであり、呼び出しサイトは通常その値を保持する。引数を保持する行、ハンドルを終了させる行、または最終使用時に通常一時値を受け取る行は、消費する。
- 理由: 付録02の安全性に関する議論 (そのR1)はこれに基づいている。`Effectful`の行を借用にしても、省けるのはsyscallの隣のRCの1組だけで、行ごとに`eml_rt`のメソッドの変更が要る。
- 適合の確かめ: 付録01 §8 G-6 (a)-(f), G-14 (スコープ: T-1からT-6およびT-9からT-12); `c_functions::a_blocking_call_argument_is_owned_by_the_waiting_continuation` (T-9) をテスト; `git show --stat <commit> -- crates/eml_core_ir/src/evidence crates/eml_core_ir/src/translate`では、この段のすべてのコミットでファイルが表示されない。
- リポジトリに残す所: docs/spec/lir.md「借用の推論」 (T-9; そのキャプチャは消費位置であり、エントリ借用された値はキャプチャされない) のキャプチャルール; docs/spec/lir.md「externの借用」 (T-3) の行ルール; docs/spec/vm.md「`blocking`の呼び出し」 (T-8) の`blocking`引数のライフタイムルール。

#### D-7: 待ちを越えてヒープの記憶を貸すときは、ページの留めを使う

- 決定: 貸す側のタスクが待ちに入った後に実行可能なコード (プールジョブ、後のネイティブな`blocking`ローアリング)にemlヒープオブジェクトへのポインタを渡すコードは、まず貸す側のスレッドで`Heap::pin`を取得し、留めが返された場合にのみ貸渡しを行い (オブジェクトはラージオブジェクトページにある)、それ以外の場合はコピーする。借用側は留めを所有し、Cが戻った後、答えを届ける前に留めを捨てる。ページは1つの留め認識関数`return_page`を通じてのみ`std::alloc::dealloc`に到達する。留められたページはパークされ、解放も再利用もされず、最後の留めがページを返却する。プールスレッドは`Ctx`やRCに一切触れず、ヒープ内で何も割り当てない。ヒープの走査は、走査する語だけを読む。`String`/`Bytes`/`UnboxedArray`のインプレース書き込みには`Heap::is_unique`が必要である。新しい種類の貸す記憶はT16を広げ、`PagePin`を使い回す。
- 理由: 静止 (quiesce)により、ヒープが回収または破棄される間、ジョブを見捨てることができる。ラージオブジェクトのみの貸渡しと留め認識の解放を用いれば、ライブネスの仮定が破綻してもリークが発生し、use-after-freeにはならない。
- 採らない案: すべてのオブジェクトのページに留めを付ける (小さな引数ごとに64 KiBを保持し、解放された小さなブロックは再利用する)。見捨てることを拒否する (Cの呼び出しがハングすると`exit`/Ctrl-Cがハングするか、REPLセッションが終了する)。パークされたページを`Control`の予算に数える (決して戻らない呼び出しがあると、セッションの制限値を永久に下げてしまう)。`FfiTake`で再検証する。
- 適合の確かめ: 付録02 §8 G-F1からG-F7。G-F5の「パスがsubmitとtakeの間にオブジェクト引数の使用を配置してはならない」とは、そこでのいかなる命令もオブジェクトの要素を読み書きしたり、そのカウントを0にしたりしてはならないことを意味する。カウントが1以上に保たれた状態でPerceusがyieldパスに配置する`dup`/`decref`は許可される。
- リポジトリに残す所: docs/spec/object-model.md「ページの留め」 (new)、T16の書き換え、T1およびT4の書き換え、新しいT17 (T-7); docs/spec/vm.md「`blocking`の呼び出し」 (T-8)。

### 5.2短・中期的設計

正当な理由があれば実装中に調整してもよい。調整内容はコミットメッセージに記録すること。

#### D-10: 推論、適応、およびパスの順序

- 決定: Contractの後、Perceusの前に新しいパス`eml_lir::borrow` (`Pass::Borrow`) を導入する: Boxing, Flatten, Contract, Borrow, Perceus, Reuse。ルールB0-B4およびA1-A3は記述通り付録01 §5.2に記載。B3は、再利用パス (T-5) と共有する唯一の述語である`reuse::source_words`を使用し、ライブネスと`may_yield`を意図的に無視する。SCCコードは反復的である。`$uniform`バリアントは留められたエントリパラメータを持つオリジナルであり、`variants::check`はBoxing内で実行されるため影響を受けない。`needs_copy`と`open`は独立している。
- 詳細: 付録01 §5.2

#### D-11: Perceus、verifier、および巻き戻し

- 決定: PerceusルールP1-P5およびverifierルールS1-S4, O1-O4 (メッセージは付録01 §5.3-5.4を参照)。Borrowの後に新しいverifierレベル`Borrowed` (`verify_borrowed`) を導入。それより前のレベルでは印を拒否する。エントリの巻き戻し行 = 所有RCパラメータ。降格した呼び出しは新しい巻き戻し地点となる。VM操作の変更はない。
- 検証済み事実: O3の「消費後の貸渡し」部分は、現在のパイプライン出力やテストに一切影響していない (4794件パス、プローブで0件ヒット)。したがって、T-3およびT-4は既存の期待値を変更しない。

#### D-12: 計測プログラム

- 決定: 付録01 §5.6に記載されている`bench/words_loop.em`(そのまま写す。先頭の2行の注釈は日本語で、ほかの文と同じくPLaMoで訳す)。現在は`rc_increments: 60000, rc_decrements: 60016`であり、T-9後は`0`と`16`となり、その他のフィールドは変更なし (導出は付録01 §5.6を参照)。

#### D-13: VMにおける`blocking`の貸渡し

- 決定: 付録02 §5単位2の記述通り。`Heap::pin`が`Some`を返すオブジェクトには`Arg::Lent(position, PagePin)`を、それ以外の場合は既存のコピーを割り当てる。`CAnswer.back`は`Vec<Option<Box<[u64]>>>` (貸渡しされた配列の場合は`None`)となる。すべての留めは、ブリッジが戻った後のジョブクロージャ内で破棄される。行、ローアリング、オペランド、逆アセンブル、および`OpKind::Job`は変更しない。
- 他のヒープのブロックおよび凍結チャンクに対する`Heap::pin`: 新しい信頼済みアイテムなしではページクラスが読み取れない場合、呼び出し元ヒープのページではないブロックについては`None` (コピー)を返し、それを要するテストを外し、「ページの留め」(付録02 §6 U1) でその旨を明記する。

#### D-14: 2つの部分の順序

- 決定: T-7およびT-8 (FFIの貸渡し)は、T-6の後、T-9の前 (Borrowの有効化)に配置する。T-9より前はどの関数も借用パラメータを持たないため、D-6は自明に保持され、T-8はスナップショットを変更しない。その後、T-9が2つの部分の両方について`tests/miri/blocking.lir`を一度だけ再生成する。D-6を`blocking`のために留めるテストはT-9に移動する (付録01では単位8に配置されていた)。
- コミットごとのチェック「`crates/eml_rt`または`crates/eml_extern`の下のファイルを変えないこと」は、T-1からT-6まで、およびT-9からT-12までのみ適用される。

## 6. 実装上の制約と規約

- コードコメントおよび`docs/`内の日本語テキストは英語で起草し、PLaMoで翻訳する (`form: plain`、コード識別子および`docs/`セクション名は`glossary`に記載)。日本語とラテン文字の間、またはインラインコードの間にスペースを入れない (CLAUDE.md)。
- コードおよび`docs/`内で`docs/plans/`やプランの識別子を引用してはならない。各仕様ルールは、それを実装するコミット内で記述する (CLAUDE.md, roadmap「進め方」)。
- 種類2 (期待値の変更) はT-9においてのみ、かつそこに記載されたスコープ内でのみ行われる。種類1の変更: なし。その他のタスクにおける期待値の変更: 停止して報告する (D-5, D-14)。
- すべての新規テストは、単独で実行した際に1秒以内に完了し、その結果がマシン速度に依存しないこと。タイミングテストは`#[ignore = "<reason>"]`とし、docs/implementation/testing.md「よく使うコマンド」に記載する。
- `eml_rt`の変更 (T-7のみ)は、`crates/eml_rt/CLAUDE.md` (D-7) のゲートを通過する。
- アドホックな`.em`は`timeout 20`で実行する。
- 各タスクの独自の基準に加え、共通のゲートを設ける: `cargo nextest run` → すべてパス、`cargo clippy --all-targets -- -D warnings` → 警告なし、`cargo fmt --check` → 終了0。

## 7. 実装タスク

| ID | タスク | 根拠 | 依存関係 | 並列化可否 |
|---|---|---|---|---|
| T-1 | ベースラインbench `words_loop` | D-12 | - | いいえ |
| T-2 | IRおよびテキスト内の印 | D-1 | T-1 | いいえ |
| T-3 | 分類、verifierの所有権、巻き戻し行 | D-1, D-5, D-6, D-11 | T-2 | いいえ |
| T-4 | 印付きPerceus | D-5, D-11 | T-3 | いいえ |
| T-5 | 共有再利用述語 | D-10 | T-4 | いいえ |
| T-6 | Borrowパス (まだパイプラインに含まれていない) | D-2, D-3, D-4, D-10 | T-5 | いいえ |
| T-7 | `eml_rt`ページの留め (T17) | D-7 | T-6 | いいえ |
| T-8 | `FfiSubmit`は大規模な引数を貸す; ドキュメント | D-6, D-7, D-13 | T-7 | いいえ |
| T-9 | Borrowの有効化 | D-2からD-6, D-10, D-11, D-14 | T-8 | いいえ |
| T-10 | ソースレベルのLIRテスト | D-1, D-2, D-10 | T-9 | はい (T-11、T-12と並べられる) |
| T-11 | VMテスト | D-3, D-4 | T-9 | はい |
| T-12 | UIおよびREPLテスト | D-2, D-3, D-4 | T-9 | はい |
| T-13 | 段の終了 | roadmap「進め方」 | T-10, T-11, T-12 | いいえ |

この段の最初のコミットは、T-1より前の本プラン (`Plan borrowed-params`) である。T-7はT-1からT-6へのコード依存関係を持たない。順序はT-8がT-9より前に完了するようにD-14によってのみ固定されている。

### T-1: ベースラインbench `words_loop`

- 目的: この段で削除される形状の本日のRCカウントを留めておく
- 対象: `bench/words_loop.em`; `crates/eml_vm/tests/bench.rs` (`PROGRAMS` 41 → 42, `upsert_by_hand`の後に挿入; `words_loop`をテスト; `unwind_points`スナップショットの新しい行); docs/implementation/benchmarks.md (行の説明のみ)
- 作業: 付録01 §5.6のプログラムをそのままコピーし、他のbenchプログラムと同様にスナップショットテストを追加する
- 実装者に任せること: なし
- 完了の条件:
  - `cargo nextest run -p eml_vm --test integration -E 'test(bench::words_loop)'` → パス。そのインラインスナップショットに`rc_increments: 60000`と`rc_decrements: 60016`が含まれている
  - `timeout 20 cargo run -q -p eml_cli -- run bench/words_loop.em` → `30000`と出力される

### T-2: IRおよびテキスト内の印

- 目的: D-1の表現。動作の変更はない
- 対象: `crates/eml_lir/src/{lib,syntax,text,pretty}.rs`、`Block`リテラル、`crates/eml_core_ir/src/lowering.rs`; `Borrowed`より前の段階は印を拒む; docs/spec/lir.md「ブロックの列」; docs/implementation/testing.md「LIRのテキストの形」
- 作業: 付録01 §5.1および §5.7 (単位2) のT-2行
- 実装者に任せること: Rustシグネチャ
- 完了の条件:
  - 新しい`text::`ラウンドトリップテストと、「Core IRのテキストが`borrowed`を拒否する」テストのパス
  - `git show HEAD | grep -c '^-.*@"'` → `0` (既存のインラインスナップショットを編集していない)

### T-3: 分類、verifierの所有権、巻き戻し行

- 目的: `for_each_consumed`/`for_each_lent`およびverifierにおけるD-1/D-5のルール、および仕様書のD-6行のルール
- 対象: `crates/eml_lir/src/{lib,verify,unwind}.rs`; docs/spec/lir.md「verifier」 (S1-S4, O1-O4, 新しいメッセージ)、「externの借用」 (O3が同一変数ルールを置き換える; 後続の行に対するD-6のルールと、どの行も変えなかったこと)、「巻き戻しの地点」; docs/spec/vm.md「巻き戻しの表」のエントリ行
- 作業: 付録01 §5.1, §5.4およびその単位3
- 実装者に任せること: 分類関数の正確なシグネチャ (`FnIdx`ごとの呼び出し先マスク、ターゲットブロックマスク、および`c_functions`が必要)
- 完了の条件:
  - 付録01 §5.4のメッセージごとに1つの新しい`verify::`テスト、および「lent field, consumed owner」 → パス
  - 新しい`unwind::`テスト: エントリの行が借用パラメータを含まない → パス
  - 既存のテスト行が削除されていない: `git show HEAD -- crates/eml_lir/tests | grep -c '^-[^-]'` → `0`

### T-4: 印付きPerceus

- 対象: `crates/eml_lir/src/perceus.rs`; docs/spec/lir.md「Perceus」 (P1-P5)
- 作業: 付録01 §5.3およびその単位4、およびそこに記載されている手書きテスト
- 実装者に任せること: コードのレイアウト以外なし
- 完了の条件:
  - `crates/eml_lir/tests/perceus.rs`の新しいテストがパスすること
  - `git show HEAD -- crates/eml_lir/tests/perceus.rs | grep -c '^-[^-]'` → `0` (追加分のみ)

### T-5: 共有再利用述語

- 対象: `crates/eml_lir/src/reuse.rs`のみ: ソースおよび`con`に使用される`pub(crate) fn source_words(&Shape) -> Option<usize>`
- 種類3 (機械的な追随); 仕様の変更なし
- 完了の条件: `git show --stat HEAD` → `crates/eml_lir/src/reuse.rs`のみがリストされる; すべてのテストが変更なくパスする

### T-6: 借用パス、パイプライン外

- 対象: `crates/eml_lir/src/borrow.rs` (`pub fn borrow`); `crates/eml_lir/tests/borrow.rs` (`crates/eml_lir/tests/main.rs`で宣言); docs/spec/lir.mdの新セクション「借用の推論」 (B0-B4, A1-A3, 留められた関数, `open`, `$uniform`ボディ) および「関数のABI」 (`$owned`, 借用されたABI); docs/implementation/testing.mdの「よく使うコマンド」 (無視されるタイミングテスト)
- 作業: 付録01 §5.2およびその単位6 (リストされたすべてのテストを含む。ルールごとに1つ; 降格; 付け替え; `!open`を伴う2関数の末尾SCCメンバーの`$owned`および`open`を伴う`$owned`; 薄い形式およびコピー形式の`$uniform`; 複数値の降格; `&f`あり・なしの`f`を共有する2つのプログラム; 印のみを主張する2000関数のチェーン) および`#[ignore = "timing: borrow on a 20000-function chain, release only"]`
- 実装者に任せること: ワークリストのレイアウト; SCCヘルパーの配置場所
- 落とし穴: SCCコードは反復的である必要がある (プログラムの深さに対するRustの再帰は不可); `$owned`のコピーはA1/A2の前にクローンされ、その後適応される; デバッグビルドでは`variants::check`のように再ビルドおよび比較される
- 完了の条件: `cargo nextest run -p eml_lir --test integration -E 'test(borrow::)'` → すべてパスする

### T-7: `eml_rt`ページの留め (T17)

- 目的: D-7の仕組み
- 対象: `crates/eml_rt/src/alloc.rs`、`Heap`メソッド (`heap/unboxed.rs`または`heap/mod.rs`)、`crates/eml_rt/src/lib.rs`、`crates/eml_rt/src/trust.rs`、`crates/eml_rt/src/heap/tests/trusted.rs`; docs/spec/object-model.md (「確保器」の下に新しい「ページの留め」; 「確保と解放」、「別のスレッドからの解放」、「回復」、「凍結した領域」の「寿命」に1行ずつ追加; 「`unsafe`の核」: `verus!`外のアイテム、T1、T4の文言変更、新しいT17)
- 作業: 付録02 §5単位1の記載通り (プロセス全体の`PINS`テーブルと`PINNED`アトミック、`return_page`を唯一のページ`dealloc`とする、`PagePin`、`Heap::pin`、テスト用の`PagePin::parked`、信頼済みテスト1-7)
- 実装者に任せること: 名称、ドロップがヘルパーを呼び出すかどうか; D-13の他のヒープ向けフォールバック
- 完了の条件:
  - `cargo nextest run -p eml_rt` → `the_trust_list_is_exactly_the_trusted_items`を含めすべてパス
  - `cargo verus verify -p eml_rt` → `0 errors`
  - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 終了0
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` → パス、リーク報告なし; 再度`-Zmiri-tree-borrows`を追加してパス; `crates/eml_rt/CLAUDE.md`の予算範囲内
  - `grep -n "dealloc(" crates/eml_rt/src/alloc.rs` → `return_page`内部およびピンのドロップパス内のみヒット
  - コミットメッセージに`bench/rt.sh`の前後の記録を記載 (ゲートではない)
- 落とし穴: `trusted::`がMiriの予算を超えた場合、予算を引き上げる前に信頼済みテスト1, 3, 4をマージすること

### T-8: `FfiSubmit`が大規模な引数を貸す; ドキュメント

- 目的: 大規模な`blocking`引数の無制限なコピーを削除する (ユーザーリクエスト)
- 対象: `crates/eml_vm/src/ffi.rs`、`crates/eml_rt/src/driver.rs:64` (`Op::Job`ドキュメント)、`crates/eml_vm/tests/{ffi.rs,ffi_abandon.rs,miri.rs}`、`crates/eml_vm/tests/miri/blocking.lir`、テストの`rs_*`フィクスチャ; ドキュメント: docs/spec/vm.md「`blocking`の呼び出し」および563, 572, 776-779, 1091行目; docs/spec/object-model.md T16および438, 559行目; docs/spec/declarations.md「`extern "C"`」 (232-235行目); docs/future/ffi.md「長く走る呼び出し」 (89-97行目)、「データの配置」 (113行目)、「却下した案」; docs/future/async.md:243, 261, 274; docs/future/multicore.md:84; docs/implementation/testing.md:227, 286; docs/implementation/status.md:211
- 作業: 付録02 §5単位2およびD-13。vm.mdのテキストには`blocking`引数に対するD-6のライフタイムルールが記載されている (lir.mdの捕まえる規則はT-9が書く。このコミットでは、vm.md自身に規則を書く)
- 実装者に任せること: フィクスチャ関数; `copy_args`のリネーム
- 落とし穴: `miri::a_blocking_call_lends_a_large_array_on_the_test_driver`はポインタの同一性しか比較できない。スロットの整数をポインタに戻す呼び出し先は`-Zmiri-strict-provenance`の適用対象外であるため (`crates/eml_vm/tests/miri.rs:628-631`)。`blocking.lir`を拡張する場合は、パイプラインから再生成する必要がある (`miri.rs:640-648`の等価性アサーションにその方法が記載されている)
- 完了の条件:
  - `cargo nextest run -p eml_vm -E 'test(ffi::) | test(ffi_abandon::)'` → すべてパス。これには`ffi::a_blocking_call_receives_a_large_array_itself` (このコミット前は失敗)、`ffi::a_blocking_call_receives_a_copy_of_a_small_array`、および`eml_rt::PagePin::parked(pointer)`をアサートする`ffi_abandon`テストが含まれる
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` → パス。ここでも`-Zmiri-tree-borrows`を適用
  - `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=address CARGO_TARGET_DIR=target/asan cargo test -Zbuild-std --target aarch64-apple-darwin -p eml_vm --test integration -- ffi:: ffi_abandon::` → パス。ASanやリークの報告なし
  - CLAUDE.mdのfeature-combo clippy行 → クリーン
  - 既存のスナップショットの変更なし: `git show HEAD | grep -c '^-.*@"'` → `0`
  - 実装者による手動確認: 上記リストのファイルで`grep -rn "blocking" docs crates/eml_vm/src crates/eml_rt/src crates/eml_io/src`のヒットをすべて読み、ジョブがコピーのみを保持している、あるいはヒープポインタを貸していないという記述が残っていないことを確認
  - コミットメッセージに前後のプローブテーブル (付録02 §5単位2) を記録する (ゲートではない。想定: 引数1つにつき8 KiB以上の行は、最後の列の値がほぼなくなる)

### T-9: Borrowをオンにする

- 目的: 段の効果。種類2 (期待値の変更) を伴う唯一のタスク
- 対象: `crates/eml_lir/src/pipeline.rs` (`Pass::Borrow`, `verify_borrowed`)。以下にあるすべての種類2の期待値を`cargo insta test --accept`で再生成し、レビュー済み。`crates/eml_vm/tests/miri/blocking.lir`を再生成。`crates/eml_core_ir/tests/c_functions.rs`に`a_blocking_call_argument_is_owned_by_the_waiting_continuation`を追加 (付録01 §5.7単位8のテキスト)。仕様: docs/spec/lir.md「パスの順番」 (6つのパス; "借用パラメータの最適化はしない" を削除)、「末尾の位置の呼び出し」 (降格の例外と上限)、lir.md:64および :608、「借用の推論」にD-6のキャプチャルールを適用。docs/spec/core-ir.md「変換の規則」 射影の注記。docs/spec/vm.md「呼び出しと戻り」、および「`blocking`の呼び出し」でキャプチャルールにlir.mdを引用。docs/future/native.md「呼び出し規約とエントリ」 (借用ABI) および「展開と最適化の前提」 (3.7〜4.0倍の数値のもとの形を "a callee that owns the string and is called per iteration" に書き換え、`words_loop`を指させる; 数値は維持)。docs/implementation/benchmarks.md:844で同様の書き換え。docs/spec/object-model.md:336。docs/implementation/status.md:62
- 種類2のスコープ (理由: "借用パラメータはRC命令を移動または削除し、`$owned`関数を追加し、呼び出しを降格させる"): `crates/eml_core_ir/tests/`配下のスナップショットで`Pass::Perceus`または`Pass::Reuse`時にLIRを出力。`crates/eml_lir/tests/`のテストで`passes_until(.., Pass::Perceus | Pass::Reuse)`を経由。`crates/eml_vm/tests/bench.rs`の`RunStats`スナップショットおよび`unwind_points`。`crates/eml_vm/tests/codegen.rs`のソースからコンパイルされたプログラムの逆アセンブル。`crates/eml_vm/tests`の他の箇所にあるソースプログラムから`RunStats`のカウントをアサート。`crates/eml_vm/tests/miri/blocking.lir`
- 実装者に任せること: 再生成された各値をD-2からD-4に照らしてレビューすること以外なし
- 完了の条件 (BASE = T-8のコミット):
  - C1 `cargo nextest run -p eml_vm --test integration -E 'test(bench::words_loop)'` → `rc_increments: 0`, `rc_decrements: 16`でパス
  - C2 `paste <(git show BASE:crates/eml_vm/tests/bench.rs | grep -o 'rc_increments: [0-9]*' | cut -d' ' -f2) <(grep -o 'rc_increments: [0-9]*' crates/eml_vm/tests/bench.rs | cut -d' ' -f2) | awk '$2 > $1 {bad++; print} END {print NR " rows"; exit bad != 0}'` → 正確に`45 rows`を出力し、exit 0
  - C3同コマンドで`rc_increments`を`reuses`に、`$2 > $1`を`$2 != $1`に変更 → 正確に`45 rows`を出力し、exit 0
  - C4 CLAUDE.mdのfeatureの組み合わせのclippyの行 → 警告なし
  - `cargo nextest run -p eml_core_ir --test integration -E 'test(c_functions::a_blocking_call_argument_is_owned_by_the_waiting_continuation)'` → パス
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` → パス。再度`-Zmiri-tree-borrows`を適用。コミットメッセージに前回のコミットに対する`blocking.lir` Miriタイムを記載
  - `git show --stat HEAD -- crates/eml_rt crates/eml_extern crates/eml_core_ir/src/evidence crates/eml_core_ir/src/translate` → 何も出力しない
  - `git show --stat HEAD -- crates/eml_vm/src` → 何も出力しない
- 落とし穴: C1で`rc_increments`が0以外の場合、読むだけの関数がまだ`s`を所有している。LIRをダンプして印を確認。0で`rc_decrements`が16でない場合、`s`の解放タイミングがずれている。bench.rsでの`peak_objects`の増加は種類2として許容される (借用された値は呼び出し後に呼び出し元によって解放される)。コミットメッセージにその旨を記載すること。

### T-10: ソースレベルのLIRテスト

- 対象: `crates/eml_core_ir/tests/perceus.rs`
- 作業: 付録01 §5.7単位8のテストのうち`c_functions`を除くもの (T-9に移動): `words_loop`の形の読むだけの関数は`borrowed`されておりRCがない; `f p.name`の後に`p`の使用がある (`f`が読み取る際は`dup`がなく、消費する際はある); `let u = if c then s else t`は読み取りのみ (両方およびmergeパラメータが借用済み); yieldの地点をまたいで生存するパラメータは所有され、継続のクロージャに移動される; `Int`リストに対する`map`は`reuse`を維持する
- 完了の条件: `cargo nextest run -p eml_core_ir --test integration -E 'test(perceus::)'` → すべてパス; `git show HEAD | grep -c '^-.*@"'` → `0`

### T-11: VMテスト

- 対象: `crates/eml_vm/tests/scaling.rs`, `crates/eml_vm/tests/bench.rs` (`unwind_points`の隣に新しい`owned_variants`スナップショット); docs/implementation/benchmarks.md「回数のテスト」
- 作業: 付録01 §5.7単位9: (a) 末尾呼び出しのサイクル`lambda -> g -> tail apply`が各ラウンドで新しい`String`を読むだけの関数に渡し、1,000ラウンドと100,000ラウンドで`peak_stack`が等しくなる; (b) `words_loop`の形、2つのサイズで`rc_increments`が等しい; (c) `bench::owned_variants`、ベンチプログラムおよびスクリプトごとの`$owned`関数名のインラインスナップショット (`wc`, `grep`, `logstat`, `csv2jsonl`, `deploy`); 合計をコミットメッセージに記載
- 完了の条件: `cargo nextest run -p eml_vm --test integration -E 'test(scaling::) | test(bench::owned_variants)'` → すべてパス

### T-12: UIおよびREPLテスト

- 対象: `tests/ui/run/...`, `tests/ui/run-fail/...`, `crates/eml_cli/tests/repl/`
- 作業: 付録01 §5.7単位10 (値として使い部分適用する、読むだけの関数; 2度再開するhandlerの下の、読むだけの関数; 呼び出し先が再開可能な操作を行う、降格した複数値呼び出し; `run-fail`が借用の引数を持つ読むだけの関数の中でパニックを起こす; REPL: 読むだけの関数を定義し、後の入力で値として渡し、新しい値で末尾呼び出しを行う)
- 完了の条件:
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::)'` → すべてパス
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` → パス; 再度`-Zmiri-tree-borrows`を適用

### T-13: 段を閉じる

- 作業: docs/future/roadmap.md「進め方」の手順を順に、T-12後のクリーンなツリー上で1つのコミット`Close borrowed-params`として実施:
  - 計測: まず`bench/scripts.sh` (行が範囲外になった場合は別コミット`Resize bench rows`で行をリサイズ)、次に`bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh`を実行し、docs/implementation/benchmarks.md「今の数値」の3つのテーブルを置換
  - docs/implementation/status.md (借用パラメータの実装完了; `blocking`の大引数貸す処理)、docs/implementation/architecture.md (新しいパス`borrow.rs`、ページの留め)
  - docs/future/roadmap.md: `borrowed-params`項目を削除。`jit-core`の「前提」で段名を項目名に置換。「その後の項目」に追加し、それぞれに「後に回した理由」と「見直す条件」を記載: 同じブロックの引数から借りるブロック引数 (理由: どのパスでも生成されない; 見直し: ループ化や他のパスにより、同一ブロックの引数のみを読み取るループヘッダー引数が生成される場合)、薄い形の`$owned` (理由: コピーは末尾サイクルでフレーム不要; 見直し: `bench::owned_variants`がプログラムごとに数個を超える場合、または`bench/run.sh`のロード時間が変動する場合)
  - docs/overview.mdとdocs/README.mdは変更があった場合のみ更新。CLAUDE.mdはルールやポインタが変更された場合のみ更新 (Architectureパスリストにパス名がある場合はBorrowを追加)
  - セクション5のすべての決定が`docs/`に収まっていることを確認した上で、`docs/plans/2026-11-11-borrowed-params.md`と`docs/plans/2026-11-11-borrowed-params/`を削除
  - コミットメッセージ: 何をどこへ移動させたか、テスト変更の種類と理由 (T-9では種類2)、顕著な計測値の差異、延期日
- 完了の条件:
  - `grep -rn "借用パラメータの最適化はしない" docs/` → 出力なし
  - `grep -rn "docs/plans" crates docs --include=*.rs --include=*.md` → 出力なし
  - `ls docs/plans` → 該当ファイルがないか空
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → すべてパス

## 8. 実行順序

厳密に順次: 計画コミット → T-1 → T-2 → T-3 → T-4 → T-5 → T-6 → T-7 → T-8 → T-9 → {T-10, T-11, T-12} (任意の順序。並列ワークツリーで実行してもよいが、一度に1つずつ確定させる) → T-13。

## 9. 検証計画

- ベースライン (Base時点、計画コミット前): `cargo nextest run` → すべてパス (f918f5fbのスクラッチクローンに追加プローブを適用した結果、4794件がパス、21件がスキップ。正確な件数は実行者のベースライン)、`cargo clippy --all-targets -- -D warnings` → クリーン
- 全体検証 (T-12後、T-13前): `cargo nextest run` → すべてパス、`cargo nextest run --profile timing -j 1` → 低速と判定されたテストなし、`cargo clippy --all-targets -- -D warnings`およびCLAUDE.mdのfeature-combo clippy行 → クリーン、`cargo fmt --check` → 終了コード0、`cargo verus verify -p eml_rt` → エラー0、CLAUDE.mdの`eml_rt trusted::`および`eml_vm miri::`のMiriコマンド → パス、BASE = T-8コミット のT-9のC1-C3 → 記載の通り
- ユーザーによる手動チェック: 不要。CIでLinux ASan/TSanおよびWindowsをカバー

## 10. リスクと未解決事項

| ID | 項目 | 影響 | 解決方法 / 時期 | 未解決時のデフォルト |
|---|---|---|---|---|
| Q-1 | `Heap::pin`は、新しい信頼済みアイテムがない限り、他のヒープや凍結チャンクのブロックのページクラスを読み取れない | T-7, T-8で共有文字列および定数文字列のテストを実施 | 実装者、T-7にて | そのようなブロックには`None` (コピー) を返し、テストを削除し、ドキュメント化 (D-13) |
| Q-2 | ピークメモリ: 借用した値が、呼び出し後に呼び出し元によって解放される | T-9の`peak_objects` | T-9レビュー | コミットメッセージに理由を添えて、種類2 (期待値の変更) として受理 |
| Q-3 | B3が過大評価している (同じ語数の到達可能な`con`であればどれでも) | 借用回数を減らす可能性あり | 実測された損失がある場合のみ | 維持 |
| Q-4 | `blocking.lir`のMiri時間が`$owned`コピーに伴い増加する | T-9 Miriステップ | T-9 | 時間を記録し、Miriの実行がCIの予算を超えた場合は停止して報告 |
| Q-5 | C1の期待値が推論されている | T-9 | T-9の落とし穴による診断 | T-9の落とし穴に従う |

## 11. 実行中の決定

計画を細かくする決定である。5〜7や付録と食い違うときは、こちらに従う。

- T-2で足した、どの段でも印を拒むverifierのテストは、途中の状態を固定していた。T-3は、これを`Borrowed`より前の段に絞った。このテストは基準の時点になかったので、種類1 (成否の変更) ではない
- 分類の文脈は、traitでなく具体的な構造体`Lends` (`Lends::new`、`Program::lends()`) である。既存のテストの呼び出し箇所は、種類3として新しいシグネチャに合わせた
- Perceusは、直接の呼び出しがフィールドを貸す前に、保持している持ち主を片付ける。保持しているものが呼び出しをまたぐことはない (docs/spec/lir.mdの「持ち主を手放す時点」)
- 規則B1は、参照を数える変数だけを数える。スカラーの変数やフィールドを消費しても`dup`は要らないので、その根は所有にならない
- `$owned`の写しをdebugで作り直して比べる確かめは、失敗しえないので外した。名前の規則 (D-3) は、`crates/eml_lir/tests/borrow.rs`の2つのプログラムのテストと、REPLの`Mismatch`の確かめで確かめる
- Q-1は解決した。`Heap::pin`は、runのほかのヒープのブロックと、付けた凍結した塊のブロックにも使える。T-8は、共有の文字列と定数の文字列のテストを残す
- `PINNED`は`Relaxed`の順序を使う。実行系の核は`SeqCst`を使わない。留めたページが`return_page`に届く道は、どれも留めた側から前に起きる辺 (共有のrcのAcqRelとAcquire、`RemoteSink`のAcquireの取り出し、凍結した塊の`Arc`を捨てること、workerのjoinと`quiesce`) を通る (docs/spec/object-model.mdのT17)

## 付録: レビューによる主な変更点

- ロードマップの`wc`のバイトごとの`dup`は今はもうない。ユーザーは新しいプログラム (`words_loop`) で測定し、段を続けることを選んだ → D-12
- 初期設計では末尾SCCの`$owned`コピーが無効だった → コピーにA2を適用; D-4
- FFI設計のr1は、すべてのオブジェクトの64 KiBページを留め、ライブネスのみに依存していた → 大規模オブジェクトのみ、留めを確かめる`release_large`を採用。失敗モードはリーク (D-7)
- 2つの設計が`eml_rt`の変更および`blocking.lir`で競合していた → D-14 (コミットごとのチェックの順序と範囲)
