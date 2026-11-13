# エフェクトをネイティブで動かす段 (jit-effects)

- 作成日: 2026-11-11
- 基準: 前の段 (jit-externs) の`Close jit-externs`のコミット。付録の`ファイル:行`の参照はmain @ 432fffb4の状態を指す。基準とのずれは、段を始めるときに最初の作業で確かめる。行の番号は手がかりで、名指した項目が決める
- 状態: 設計の段階の計画。段を始めるときに、前の段の結果に合わせて実行できる計画へ書き直し、この段の最初のコミットで置き換える。付録は`docs/plans/2026-11-11-jit/`にある。この段の設計は付録05の§1.1と§5.1 (あわせて§1.0、§3.1、§6) で、付録00と食い違う所は付録00に従う

## 1. 目的と完了の条件

- 目的: すべてのエフェクトのprim、`Call::Clause`、abortのlanding、yieldの遅い道、`eml$raise`からのハンドラの呼び出し (`try_panic`、`try_io`) をネイティブで動かす。evidenceの役割を`Ctx`の面に移し、eml_vmとeml_rtの補題が1つの定義を使うようにする (rt-layout G-10)
- 完了の条件 (付録01の§5.3): 断る形のヒストグラムに残るのが、async-classの行、blocking C、`par` (と`child_abort`) だけであること。UIのカテゴリ`effects`、`unwind`、`panic`に断られるファイルがないこと。eml_rtのゲート (N-29) が通ること。エフェクトのbenchのネイティブの数値をbenchmarks.mdの「ネイティブの数値」に記録すること

## 2. 範囲

- 範囲に入るもの: エフェクトのprim (`yielding`と`yield_status`はインライン、ほかはprimごとに1つの`eml_rt_<prim>`エントリ)。名前で引くモジュールのデータとしてのエフェクトの番号とマスクの列。`Call::Clause`。landing。yieldの遅い道。`eml$raise`からの節の呼び出し。rootのprim。`CTX_YIELD_ABORT`。eml_rtのevidenceの役割と構築子。`eml_lir::fork_plan`。eml_schedの`ForkTables`と`fork_evv`。eml_vmをそれらに通し直すこと
- 範囲に入らないもの: `child_abort`、`par`、`compute`、async-classの行、blocking C、上限の種別、emlのコードに入り直す行の再raiseの要求 (jit-tasks)。REPL (jit-repl)。`empty_mut_array` (jit-externsで入っている)

## 3. 前の段から受け取るもの

名前は付録のものである。書き直すときに、基準のコードと照らし合わせる。

- jit-rt: `eml_enter`と`enter() -> Result<(), Fatal { code, site }>`、何も記録しない`eml_fatal(ctx, code, site)`、`entries!` (`plain`と`shim`)、`CTX_*`の定数 (`Native`はずれ0、`heap`は72)、`ctx.runtime()`と`set_runtime`
- jit-core (付録03の§2): `lower/unwind.rs`の`after_unwind_point(fx, &Point, After)`と`After::{Call, Faults, Arith, Never}`。`eml$raise(ctx, function, site, purity)`と`eml_nrt_failed(ctx, function, site, purity) -> action` (`eml_nrt/src/services.rs`。`function`はコードのアドレス、`site`はその関数の`FnRow.sites`の添字)。`worker::record(ctx, RunFault, Option<At>) -> Fatal` (`FATAL_RECORDED = 3`) と`worker::park_panic`。`Fault::Engine`。名前ごとのデータのシンボル (`Datum`、`consts.<kind>() -> DataId`、`DatumRow.slot`)。`mem.rs`、`abi.rs`、`thunk.rs`。`bench/native.sh`
- jit-coreの残した確かめ: `Faults`と`Never`の地点の後始末は、jit-coreでは`eml$raise`がいつも致命的な誤りで終わるので、コードは出すが実行されない。この段で`eml$raise`がハンドラへ戻るようになったら、`eml$raise`の後にその後始末を流す実行時のテストを足す (jit-coreのK13の査読のA-2)。handlerを外す地点のネイティブの後始末は、VMの巻き戻しと同じく、外すhandlerの状態を`ev_take_state`で取り出して手放す (`eml_vm/src/vm.rs`の巻き戻しの`ev_take_state`と`decref`。docs/spec/vm.mdの「巻き戻し」)。VMでは、これは状態を早く解放するためだけで、取り出さなくても状態はevidenceと一緒に解放される。だからrunの終わりのcensusでは、取り出したかどうかを見分けられない
- jit-apply: クロージャ (タグは足りない引数の数、走査する語0は`c<k>:<f>`のコード)、`Datum::Fn`、`consts.fn_value(FnIdx)`、`FnRow.value`、ロードした`meta`の語を受け取る`mem::closure_code(b, clo, meta)`。jit-applyは節の関数に型の付いたエントリを作らない。`apply::arity`は`function.clause.is_none()`をassertし、`ClosureEntries::id`と`define_closure_entry`はどのエントリにも一様なシグネチャ`uniform(n)`を付ける。この段はそのassertを外し、その3か所で節の関数のエントリにopのシグネチャを付ける (付録05の§5.1のN-18の一覧)
- jit-externs: すべての行を持つ`extern_rows!`、`LISTED`、`empty_mut_array`、`GateHost`

## 4. 決定

IDは付録05のものをそのまま使う。全文は付録05にあり、食い違う所は付録00に従う。

- この段が頼る契約の決定 (付録01): §1.4のエフェクトと割り込みの契約 (速いチェックは出すコードに、coldの塊は地点ごとに、判断はすべて共有のルーチンに置く)。serviceのシグネチャを変えるときの§1.4の手順 (計画に載せ、同じコミットでdocs/spec/native.mdとすべての呼び出し元を直し、記述子のテストN-7を通す)。N-9、N-12、N-18、N-24、N-29、N-38、N-39。付録00のC-2とC-6が、地点のシグネチャと下のデータのシンボルの名前を決める
- **D-1** (付録05の§1.1 D-1) エフェクトのprim: `prim yielding()`と`prim yield_status(ev)`はインラインで、`CTX_YIELD_TARGET`と`CTX_YIELD_ABORT`を読む。ほかのprimは、同じ名前の`Ctx`のメソッドを呼ぶ`eml_rt_<prim>`エントリを1つずつ持ち、普通のshimの本体 (`catch_unwind`を含む。U4) を使う。エフェクトの番号はデータのシンボル`e:<effect>`から読む (C-6。呼び出しの隣で1回のロード)。`n_ops`は即値のままにする。マスクの列はそれぞれ、中身で名前を付けたデータのシンボル`m:<mask>`で、eml_nrtがロードのときにマシンの番号で埋め、VMと同じ順に並べる。渡すのはポインタである (`eml_rt_evv_mask(ctx, w, list)`)。`prim yield_abort`の後は`After::Never { raise: None }`にする。`CTX_YIELD_ABORT`は最初に読む所と一緒に入れ、N-29の普通のゲート (+0.1%以下) を通す
- **D-2** (§1.1 D-2) 呼び出しである巻き戻しの地点: 変えるのは`after_unwind_point`だけである。`abort`のロードは`target`のテストの先 (`split`) にだけ置く。yieldは速い道に戻り、LIR自身の`yielding`のチェックが遅い道を取る。`split`はcoldにしない。landing `v`があるときは、`land`で`eml_nrt_landed(ctx)`を呼び、`other`で`eml_rt_ev_take_state`を呼んで`decref`し、後始末へ跳ぶ
- **D-3** (§1.1 D-3) 節の呼び出し: `mem::load_meta(c)`でロードした`meta`の語を渡して読んだ`mem::closure_code(b, c, meta)`を、`abi.rs`が作るopのシグネチャで`call_indirect`し、その後に`After::Call`を置く。節はいつも引数が揃っているので、タグは調べない。末尾の節の呼び出しは`return_call_indirect`である。`fork$E.op`の関数には、jit-tasksで`fn_value`を通して届く
- **D-4** (§1.1 D-4) `eml$raise`と`eml_nrt_failed`: シグネチャを`eml_nrt_failed(ctx, function, site, purity, out) -> i64`にする (C-2の形に`out`を足したもの。`out`は`eml$raise`のスタックにある2語の場所)。順は次のとおりである。まず`take_fault()`。`Fault::Engine`なら要求の枝 (この段では空で、要求がないことを`debug_assert!`する。jit-tasksが埋める)、次にpanicの語を取る。ほかのfaultは、捕まえられないものか純粋さのフィルタで外れるものなら、ハンドラを探す前に記録して`Err(Fatal)`を返す。そのあと`ev_find`をし、見つからなければ記録して失敗する。見つかれば`[clause, ev]`を`out`に書き、`RAISE_PANIC = 0`か`RAISE_IO = 1`を返す (`RAISE_DONE = 2`はjit-tasksの要求からだけ来る)。`eml$raise`は、`Prelude.Panic.panicked`か`Prelude.IO.io_failed`のシグネチャで節を呼ぶ。プログラムにないエフェクトの枝は出さない。メッセージの引数は、coldの道で`eml_nrt_raise_arg(ctx, i)`から取る。記録は`At`を付けて`worker::record`で行い、返す`Fatal`はjit-coreが決めた形のままにする (C-2)。`eml_fatal`はshimが行う
- **D-5** (§1.1 D-5) rootのprim`eml_rt_root_get`と`eml_rt_root_set`は、fatalだけで失敗する。`root_set`の`Err(RootMisuse)`は速いモードの`Err`の一覧 (`WrongKind`、`StaleToken`、`RootMisuse`、`Unrecoverable`) に入る。LIRの文のコード生成のテストで確かめ、jit-replが使う
- **D-6** (§1.1 D-6) evidenceの役割: eml_rtに、specと定数の対`EV_TABLE`、`EV_STATE`、`EV_OUTER`、`NODE_EV`、`NODE_NEXT`と、`pub`の項目`Heap::alloc_fork_table`、`Heap::alloc_fork_tail`、`Ctx::fork_ev`、`Ctx::fork_of`、`Ctx::evv_effects`を足す。eml_lirに`fork_plan(&Program) -> Vec<ForkPlan>`を、eml_schedに`ForkTables`と`fork_evv`を足す。eml_vmの`compile/forks.rs`と`vm.rs`はそれらを呼び、結果はバイト単位で変わらない
- **D-18** (§1.1 D-18のjit-effectsの部分) 閉じるときの測定: `bench/native.sh`の結果で「ネイティブの数値」を置き換え、`bench/{state, state_loop, fold_state, ep_float, iter_generate, iter_pipeline}.em`と、あればその`_by_hand`の対を記録する。記録であって、比のゲートではない (N-39)
- 費用のモデルと見送った最適化 (N-38): 付録05の§1.0 (巻き戻しのチェック、yieldの地点、フレームを越えるyield、`perform`、ハンドラを入れることの行) と§3.1 (EPのチェックを巻き戻しのチェックにまとめること、`ev_lookup`と`ev_clause`を1つにしたエントリ、検証済みのprimで`catch_unwind`を省くこと、地点ごとの`perform`のキャッシュ)。書き直すときに、実行できる計画へ写す

## 5. 作業の単位の見出し

付録05の§5.1「Work units」。1つずつ1コミットにし、この順に進める。

1. 計画: 実行できる計画でこのファイルを置き換える。行の参照のずれを確かめ、基準の測定をする
2. eml_rtの役割と`CTX_YIELD_ABORT`。ゲート (N-29の普通のもの) はVerus、verusfmt、Miri `trusted::`、`bench/rt-gate.sh` (ゲートの対象の行ごとに+0.1%以下)
3. eml_rtの構築子と読み出し、そのテスト (割り当てるものはMiri `trusted::`)
4. `eml_lir::fork_plan`、eml_schedの`ForkTables`と`fork_evv`、eml_vmを通し直すこと (種類(3)、G-10のgrep)
5. インラインの`yielding`と`yield_status`、primのエントリ、データのシンボル`e:`と`m:`。コード生成のテスト
6. 節の呼び出し (D-3)。テスト: その場の節、「under」の形、末尾の節
7. D-2のabortのテスト、landing、`eml_nrt_landed`。テスト: 所有する値とトークンを持つ3つのフレームを`never`のabortが越え、`prompt$`でlandingする (census)。再開できるyieldが3つのフレームを越える。multi-shot。mask。`target`のロード、`abort`のテスト、EPのチェックを示すCLIFの固定`a_yield_point_call`
8. D-4。テスト: インラインの桁あふれ、添字の行、`panic`の行を囲む`try_panic`。開くのに失敗する所を囲む`try_io`。`Stopped`のfaultは決して捕まらない。ハンドラがないときはVMと同じ文になる
9. rootのprim
10. ネイティブのUIのsnapshot (種類(2)) とヒストグラム
11. 段を閉じる: VMのbenchの表 (eml_rtとeml_vmを変えたため) と、D-18のネイティブの測定。この計画のファイルを消す

## 6. 既存のコードの変更と費用

N-18の一覧 (付録05の§5.1):
- `after_unwind_point` (`Call`の場合)
- `eml$apply$m`の`n < m`の腕にある`Call`の確かめのインラインの写し (`lower/apply.rs`の`define_apply`、jit-apply)。最後でない部分では中断だけが起きることに頼り、`target != UNIT`を中断として扱うので、`after_unwind_point`と一緒に変える
- `apply::arity` (`clause.is_none()`のassert)、`ClosureEntries::id` (どのエントリも`uniform(n)`で宣言する)、`define_closure_entry` (`lower/apply.rs`、jit-apply): 節の関数の型の付いたエントリ`c<k>:<g>` (D-3)
- `eml$raise`の本体。`eml_nrt_failed`のシグネチャ (`out`を足す) と本体。新しい`eml_nrt_landed`と`eml_nrt_raise_arg`
- データのシンボル`e:<effect>`と`m:<mask>`、その`DatumRow`の種類 (C-6。付録05の「`Image`の行`effects`、`masks`」はこれに置き換わる)
- `Unsupported`から`Form::Prim` (`ChildAbort`を除く)、`Form::Clause`、`Form::Prompt`を消す
- eml_rt: `CTX_YIELD_ABORT`、役割、構築子。evidenceの補題は役割の定数を通して書き直す (費用は測っていない。この段のいちばんの危険)
- eml_vmの`compile/forks.rs`と`vm.rs`の`fork_evv`と`child_abort`を、共有の構築子に通し直す (バイト単位で同じ。`RunStats`のsnapshotは変わらない)

## 7. テストとテストの変更の種類

- 新しいテスト: `eml$raise`が戻った後に`Faults`と`Never`の地点の後始末を流す実行時のテスト (3節のjit-coreの残した確かめ)、handlerを外す地点の後始末が状態を手放すことを、runの終わる前に確かめるテスト (代役のthunkを通して、状態に入れた物体が地点を抜けた直後にもう生きていないことを`Heap::live_objects`で見る。runの終わりのcensusでは見分けられない)。単位5〜9のコード生成のテスト (`crates/eml_native/tests`。LIRの文 → JIT → `eml_enter` → 結果とcensus)、CLIFの固定`a_yield_point_call`、eml_rtの構築子の`trusted::`のテスト
- 種類(1): 見込みはない
- 種類(2): `crates/eml_native/tests/placement.rs`の`placement::indirect_calls_are_only_in_bridge_apply_and_thunk`の期待するファイルに、節の呼び出し (D-3の`call_indirect`と`return_call_indirect`) を下ろすファイルを足す。`docs/implementation/architecture.md`の「`eml_native` の内部」の`call_indirect`の規則も同じコミットで直す。節の関数は呼ぶ先が実行時の値だからである
- 種類(2): ネイティブのUIの2つのsnapshot (流した数、断った数、ヒストグラム)。付録03の数から`prim`、`clause`、`prompt`を引いた見込みは、断られる`run`のファイルが約150、受け入れられるものが約479である。下限ではなく、書き直すときに測る。`effects/`、`unwind/`、`panic/`で断られるものは0
- 種類(3): eml_vmを共有の構築子に通し直すこと (期待値はすべてバイト単位で同じ)

## 8. 書くdocs/specの節

どれも、その規則を実装するコミットで書く。
- docs/spec/native.md: 「エフェクト」 (primの表、データのシンボル)、「節の呼び出し」、「巻き戻しの地点」 (yieldとabortの分かれ方)、「extern の失敗」 (種別とその順)、「エントリ」 (速いモードの`Err`の一覧)
- docs/spec/object-model.mdの「evidence の3つの物体」と「基本操作の契約」
- docs/spec/vm.mdの「`par` の fork と join」
- docs/implementation/architecture.md、testing.md、benchmarks.md、status.md
- docs/future/native.mdのうちこの段が実装する部分は、消すか、まだ先のことだけに縮める (付録01の§5.5 (b))

## 9. 受け入れの条件

- CLAUDE.mdのすべてのゲート。N-29のeml_rtのゲート (Verus、verusfmt、Miri `trusted::`、`bench/rt-gate.sh`) を通り、eml_rtを変える作業のコミットメッセージにゲートの表がある
- ヒストグラムに残るのが`AsyncRow`、`BlockingC`、`Par`だけである (S2bの順では、`Prim(ChildAbort)`を持つファイルは先に`par`に数えられる)。`effects/`、`unwind/`、`panic/`で断られるものが0
- eml_vmの役割の数をさがすrt-layout G-10のgrepが空である (N-24。正確なコマンドは書き直すときに書く)
- N-24のレビュー: メモリに触る1つのモジュールが、evidenceを読み書きする関数を出していない
- CLIFの固定`a_yield_point_call`があり、`target`のロード、`abort`のテスト、EPのチェックを示す
- `grep -rn 'eml_fatal(' crates/eml_nrt/src | grep -v -e src/entries.rs -e src/enter.rs` → 出力なし (R-10)
- エフェクトのbenchのネイティブの数値が「ネイティブの数値」にある

## 10. 危険・代わりの手・未解決

| ID | 項目 | 既定と扱い |
|---|---|---|
| 危険 | evidenceの補題の名前を変える費用 | 役割は、証明の中でだけspecの関数で名指す |
| 危険 | エフェクトの多いプログラムのネイティブの速さが見込みより遅い | D-18の数値に出る。まだ使える最適化は§3.1にある。比のゲートは置かない |
| 代わりの手 | abortのテストを速い道に入れる案、yieldの道全体をcoldにする案 (付録05の§3) | 採らない。記録した数値があるときだけ見直す |
| 未解決 | `m:<mask>`のデータの物体の形。付録03のD-9はデータのシンボルをどれも8バイトの場所にし、付録05のD-1は列をeml_nrtが知らせる大きさで宣言する | 書き直すときに決める。既定は付録05 (配置はeml_nrtが持って大きさを知らせ、eml_nativeは宣言だけをする) |
| U-S4-9 | §3.1の項目を見直すこと | 見送る。閉じるときに記録した数値と、特別な場合の要らない設計があるときだけ |
| U-S4-6 | ネイティブの`RunStats` | VMだけの数はゼロのまま |
| 付録00の§E | 書き直すときにこの段が決めなければならない項目 | 付録00の§E「gaps for the later stages' refresh」にこの段の項目として並べたものを、実行できる計画に書き直すときにすべて決める |
