# asyncでないexternの行とblockingでないユーザーのC関数を、すべてネイティブで呼ぶ段 (jit-externs)

- 作成日: 2026-11-11
- 基準: 前の段 (jit-apply) の`Close jit-apply`のコミット。付録の`ファイル:行`の参照はmain @ 432fffb4を指す。そこからのずれは、段を始めるときに最初の作業の単位が確かめる。行の番号は手がかりで、名指した項目が決める
- 状態: 設計の段階の計画。段を始めるときに、前の段の結果に合わせて実行できる計画へ書き直し、この段の最初のコミットで置き換える。付録は`docs/plans/2026-11-11-jit/`にある。`04-jit-externs.md`がこの段の設計で、`00-decisions.md`がそれに優先し、`01-contract.md`が長期の契約である

## 1. 目的と完了の条件

- 目的: externの表のすべての行を、eml_schedの`extern_rows!`の一覧に1回ずつ名前で載せ、asyncでない行にはどれも、両方のエンジンが走らせる実装を1つだけ持たせる。VMの`run_extern`のmatchと専用のopの処理、eml_nrtの`eml_x_`のwrapperは、この一覧から作る。ネイティブのコードは、asyncでないstdの行と、blockingでないユーザーのC関数をすべて呼ぶ。`Std.Tls`と`Std.Gui`はeml_schedへ移す
- 完了の条件 (01の§5.3を、00のR-13と付録04のD-10で改めたもの):
  - 断った形のヒストグラムに、`Par`、`AsyncRow`、`BlockingC`、`Prim` (effect、root、`child_abort`のprimだけ)、`Clause`、`Prompt`のほかの形がない
  - `tests/ui/run`で受け入れてVMと同じになるファイルが323以上、`tests/ui/run-fail`で42以上ある。ただし、実行できる計画が1つずつ挙げる`-- native: skip`の印の分は引く (印は出ない見込み)。この下限は、01の§5.3の「約308」に代わる (R-13)
  - N-21の分割のテスト (45 / 187 / 1)、N-22の網羅のテスト、`faults`でない行が失敗しないことのテスト (付録04のD-9b) が通る
  - どのテストも届かない行の一覧 (付録04のD-10) が空である
  - `RunStats`のスナップショットとVMのすべての期待値が、バイト単位で変わらない (種類(3))
  - ネイティブの数値を記録し、ネイティブで流れるスクリプトについて`bench/scripts.sh`にネイティブの行がある (N-39)

## 2. 範囲

- 範囲に入るもの:
  - 一覧の全体 (5つの節に334行すべて) と`vm_arms`の展開。VMの専用のopが共有の行の関数を呼ぶこと
  - `tls.rs`と`gui.rs`を、featureの受け渡しと`GateHost`とともに、ファイルごとeml_schedへ移すこと
  - 一覧の`inline`でない行すべての`eml_x_`エントリ。stdの行のネイティブの下ろし (`Form::StdRow`を消す)。`Const::EmptyMutArray`
  - blockingでないユーザーのCのネイティブの呼び出し (`eml_c$`のimport、inlineの狭めの確かめ、`eml_x_c_*`と`eml_rt_*`のポインタのエントリ、engineが持つpanicのpayload。`Form::CRow`を消す)。`eml_sched::ffi`の共有のCの部品
  - spec/native.mdの「extern の行」と「C の関数の呼び出し」、および8に挙げる文書
- 範囲に入らないもの:
  - 45のinlineの行を一覧の`inline`の行として載せることと、テスト「eml_nativeのinlineの展開の表が、印の付いた行の集合に等しい」(jit-core。00のC-1)
  - 101のasyncの行の本体、`par`、blockingのC、`CSubmit` / `CTake` (jit-tasks)
  - effect、root、evidenceのprim、`clause`、`prompt`、C行のpanicを`try_panic`で捕まえること (jit-effects)
  - REPLの追加をまたぐCの表 (jit-repl)
  - containerの行とCのポインタの引数のinline化 (native-inline)。AOTが`eml_c$<name>`を結ぶ方法 (native-aot)
  - eml_rtの編集: ない (`bytes_empty`はjit-rtのR4が失敗しないものにする)

## 3. 前の段から受け取るもの

名前は付録のものである。書き直しのときにコードと照らし合わせる。

| 段 | 受け取るもの |
|---|---|
| jit-rt | `eml_nrt::entries!` (`plain`と`shim`の形。種類のトークン`obj tobj unit int enum float`と`never`をそのまま受け、関数の識別子がシンボルになる)。`Raise`、`Fatal { code, site }`、`FATAL_PANICKED = 1`、`FATAL_FAULT = 2`。`Kind`、`entries()`。ABIのechoのテスト (引数の種類の並びごとにechoのエントリを求める)。`Ctx::set_fault` / `take_fault`。失敗しない`Ctx::bytes_empty`。`abi::declare_entry` (ただ1つの`Linkage::Import`)。`bench/rt.sh`と`bench/native.sh`の行の決まり (N-39) |
| sched-split | `cells`、`error` (`RuntimeError`、faultのenum、`SourceLocation`)、`park`、`driver`、`config`を持つeml_sched。開発の間だけのfeature `eml_sched/testing` (N-41) |
| jit-core | 付録04の§1.2の文法と、展開`defs`、`names`、`entries`を持つ`extern_rows!`。種の`generic`の8行、`dedicated`の行`FloatMod`、45の`inline`の行 (00のC-1)。`LISTED: &[Listed]`と`Listed { row, section, entry, inline }`、`Section` (00のC-4)。eml_nrtによる`eml_sched::rows`と`LISTED`の再エクスポート (00のC-3)。行のshimの最後の腕が`worker::record(ctx, e, None)`で記録してその`Fatal`を返すこと (`Raise`は`RunFault`の実装を持たない) と、`crates/eml_nrt/src/enter.rs`の`FATAL_RECORDED = 3` (C-2、C-5)。`worker::park_panic(ctx, Word)`と`Fault::Engine`。`eml$raise(ctx, function, site, purity)`と`eml_nrt_failed(ctx, function, site, purity) -> action` (`function`は呼ぶ関数のコードのアドレス、`site`はその関数の中の地点の番号。C-2)。`after_unwind_point`と`After::{Call, Faults, Arith, Never}`。`rows::{INLINE, CALLED, lowering}`。`Form::{StdRow, CRow}`。`eml_x_Exit` / `eml_x_Panic`。名前ごとのデータのシンボル (`Datum`の種類、`consts.<kind>() -> DataId`、`DatumRow.slot`。`s:`、`i:`、`x:`) と`eml_sched::constants`。inlineの行を`eml_sched::rows::<First>`と比べるN-27の差分のテスト (付録03の§7.1 K17)。ネイティブのハーネス (`Engine`、`execute_native`、`-- native: skip`、`ui::run_native` / `ui::run_fail_native`とそのスナップショット、census、両方のエンジンに同じ`Linked`、`agree(text)`)。`crates/eml_cli/examples/forms.rs`。`bench/native.sh`。`NativeMachine::symbols()`。spec/native.mdの「VMとの違い」「extern の失敗」「行の下ろし方」 |
| jit-apply | クロージャ、`apply`、静的なクロージャ (`v:<function>`)。受け入れるファイルの数の起点 (432fffb4で分類器による`run`が208、`run-fail`が8) |

## 4. 決定

全文は付録にある。00の節Cは付録04の文に織り込んであり、付録04は00と食い違わない。00が記録として残り、食い違いが見つかれば00が決める。

### 4.1 この段が頼る長期の方針

- D-1: 行ごとに実装は1つ (N-22、N-21、N-8): 行の本体は、eml_schedの`extern_rows!`が定める共有の行の関数として1回だけある。VMのarm、専用のopの処理、eml_nrtのwrapper、N-27のテストがそれを呼ぶ。エンジンに固有の行は`exit`と`panic`だけである。確かめ: 網羅のテスト、wildcardのないVMの網羅的なmatch、分割のテスト (45 / 187 / 1)
- D-2: 接頭辞ごとの失敗の約束 (N-9、N-10、N-12。U-8): `faults`の行は`RunFault::Extern(f)`をparkしてplaceholderを返し、生成したコードは`ctx.native.fault`を1回読む。engineが持つpayloadは`Fault::Engine`とともにworkerの構造体に置く。ほかの`Err`と捕まえたpanicは内部の誤りの類で、shimのfatalの道で終わる。wrapperの本体は`eml_fatal`を名指さない。確かめ: `grep -rn 'eml_fatal' crates/eml_nrt/src/rows.rs crates/eml_nrt/src/c.rs`が空。狭めの確かめが要る引数を持つC行のCLIFのpin。付録04のD-9b
- D-3: エントリのABIとただ1つのimport (N-7、N-16): 実行系の関数はすべて`entries!`で宣言する。ほかのimportはユーザーのC関数`eml_c$<canonical eml name>` (emlの正規の名前)だけで、既定の呼び出し規約を使い、シグネチャは`c_signature`で作る。確かめ: `grep -rn 'Linkage::Import' crates/eml_native/src`が1つの関数に当たる
- D-4: 生成したコードにプログラム全体の番号を入れない (R-6、00のC-6、N-36): ロードのときのデータは名前ごとのデータのシンボルにする。この段はユーザーのC関数のために`c:<Module.f>`を足す (中身はその`CEntry`へのポインタ)。地点はC-2のとおり`(function, site)`で表す
- D-5: crateの依存、feature、thread-local (N-1、N-2、N-5、N-41): eml_schedは01の§1.1のとおりに`tls` / `gui` / `gui-native`を持ち、上のcrateはその3つの名前を下へ渡す。新しいfeatureは作らない。`GUI_SLOT`はeml_schedの`thread_local!`のままで、eml_nrtとeml_nativeにはthread-localを置かない。確かめ: N-2のgrep。`grep -rn 'thread_local' crates/eml_nrt/src crates/eml_native/src`が空
- D-6: 性能の前提 (U-4、U-5、N-38、N-39): 定義を1つにするためのVMの費用は受け入れる (付録04のD-2)。熱い道にはどれも費用のモデルを書く。見送った最適化は一覧にする (付録04のD-13)。数値は記録で、ゲートではない
- D-7: VMとの違い (N-43、N-25): 新しい違いは出ない見込みである。印の理由は、一覧にある違いを名指す。新しい違いが要るなら、実行できる計画の利用者に見える項目にする

### 4.2 短中期の設計

括弧の中は付録04での番号である。

- D-10 (付録04のD-1、R-1、R-5): 一覧の全体を`crates/eml_sched/src/rows.rs`に置く。節は`generic`、`dedicated`、`engine { Panic, Exit }`、`lir { Par }`、`task { 101の名前 }`の5つである。この段は4つ目の展開`vm_arms` (`run_extern`の`match ext`の全体。網羅的でwildcardがない) を足し、`names`をすべての行で埋める。文法は付録04の§1.2で、jit-coreがコミットした形のままである
- D-11 (付録04のD-2): VMの専用のopの処理は、今の`externs::*` / `Ctx::*`の直接の呼び出しの代わりに`rows::<First>::<R::M>(ctx, ..)?`を呼ぶ。22のcontainerの行は、emit.rsが1つのopにまとめる行ごとに`dedicated`の行にする
- D-12 (付録04のD-3、C-2): `inline`でない行のwrapperの本体は、どれも付録04のD-3のとおりである。`never`の行は結果なしで普通に戻る (R-3)。行のshimの残りの`Err`の腕は`worker::record(ctx, e, None)`で記録し、それが返す`Fatal { code: FATAL_RECORDED, site: 0 }`を返す。`Raise`は`RunFault`の実装を持たず、どの`Raise`の実装もworkerの構造体を要してはならない
- D-13 (付録04のD-4): 呼ぶ行の費用は、レジスタにある引数、importの`call`1回、shim (`catch_unwind`の費用は測って差がない)、inline化された本体である。`faults`の行だけ、faultの読み1回と分岐1回が加わる。`rows::extern_call_cost` (`#[ignore = "timing"]`) が、`eml_x_StrByteLength`の1回の呼び出しのnsを記録する
- D-14 (付録04のD-5): `faults`でない行は、速いモードで失敗しない。付録04のD-9bがこれを固定する
- D-15 (付録04のD-6): `Par`のほかに、emlのコードを走らせる行の本体はない。再び入るエントリは増えない (N-12)
- D-16 (付録04のD-7): `tls.rs`と`gui.rs`をファイルごとeml_schedへ移す。`&mut Scheduler<P>`を取る4つの関数は`GateHost` (仮の名前。eml_vmが実装する) でgenericにする。`eml_vm::gui_park`と`eml_vm::eml_gui`は再エクスポートになる
- D-17 (付録04のD-8、C-2、C-6): blockingでないC行は5つの手順で下ろす。inlineの狭めの確かめ (coldの側は`eml_x_c_narrow`、続いて`eml$raise(ctx, function, site, Panics)`と地点の後始末)。引数の語 (`eml_rt_*`のポインタのエントリと`eml_x_c_handle_ptr`)。`NativeMachine`が持つ名前の表を`symbol_lookup_fn`で引いて解決する、`eml_c$<Module.f>`の既定の呼び出し規約の`call`。結果 (handleなら`eml_x_c_handle_new`、続いてfaultの読み)。`park_panic`を通すpanic。`cdesc`はデータのシンボル`c:<Module.f>`のアドレスである
- D-18 (付録04のD-9): `eml_sched::ffi`は、eml_externのsliceの上で`CHandle`、`HandleCell`、`CRaise`、`narrows`、`CTables` (`link`、`commit`、`CLinks`、`CMismatch`) を持つ。テストはD-9a (網羅)、D-9b (失敗しないこと)、D-9c (分割と`as`の識別子) である
- D-19 (付録04のD-10、R-13): `Form::StdRow`と`Form::CRow`を消す。`Prim::EmptyMutArray`は`Const::EmptyMutArray`として下ろす (作る関数は`eml_sched::constants`へ移す)。下限と、どのテストも届かない行の一覧 (`forms.rs --rows`)
- D-20 (付録04のD-11、D-13): native-inlineの候補と、見送った最適化 (行ごとの`plain`のwrapper、handleのポインタのinline化、`unit`の引数を落とすこと、wrapperに地点を渡すこと、libmの`fmod`のimport)
- D-21 (付録04のD-12、R-5): asyncの行は`task`の節である。jit-tasksが、名前を同じ文法の行に書き換える
- D-22 (付録04のD-14): profilerへの備え。この段はhookを足さない。エントリとCの呼ばれる側は、普通のframeを持つ名前の付いた関数である

## 5. 作業の単位の見出し (この順に、それぞれ1コミット)

1. 書き直し: 実行できる計画がこのファイルに代わる。付録の行の参照の基準からのずれを確かめ、起点の値を流し、下限 (323 / 42) と分割の数を測り直し、どのテストも届かない行と、すべてのテストの変更を種類とともに挙げる
2. `Std.Tls`をeml_schedへ移す (D-16): feature、受け渡し、再エクスポート。種類(3)
3. `Std.Gui`を`GateHost`とともにeml_schedへ移す (D-16)。種類(3)
4. 一覧の全体と`vm_arms` (D-10): `run_extern`は前置きと`extern_rows!(vm_arms)`になる。D-9a、D-9b。種類(3)
5. 専用のopが`rows::`を呼ぶ (D-11)。種類(3)。前と後の`bench/run.sh`を記録する
6. 共有のCの部品と、空の`MutArray`を作る関数 (D-18、D-19)。種類(3)
7. `inline`でないすべての行のネイティブのエントリ (D-12): D-9c、新しいABIの形のechoのエントリ、Miriの`entries::x_*`。続いて`rows::extern_call_cost`
8. stdの行のネイティブの下ろし (D-13、D-19): `Form::StdRow`を消す。`Const::EmptyMutArray`、`fail_io`の`Never`の道、どのテストも届かない行の族のagreeのテスト、スナップショット (種類(2))、spec/native.mdの「extern の行」
9. ネイティブのユーザーのC (D-17): import、`c_signature`、inlineの狭めの確かめ、6つのエントリ、`Form::CRow`を消す、Cのコード生成のテスト、1つのCLIFのpin、スナップショット (種類(2))、spec/native.mdの「C の関数の呼び出し」
10. ネイティブの測定 (N-39): `bench/scripts.sh`にネイティブの行を足す。`bench/native.sh`は新しく受け入れたプログラムをひとりでに拾う
11. `Close jit-externs`: 文書、benchmarks.mdの「今の数値」と「ネイティブの数値」、ロードマップの「進め方」の手順、このファイルを消す。`apply_int`と`apply_float`が`Std.Env.arg`の行で動くようになるので、ロードマップの「関数の値と`apply`の道のほかの形」の見直す条件から、それらがまだ断られるという文を消す

## 6. 既存のコードの変更と費用

行の番号は432fffb4のものである。

| 所 | 変更 | 費用 |
|---|---|---|
| `crates/eml_vm/src/vm.rs:4153-4894` | 前置きは残し、matchを生成する | 約600行が約40行と一覧の行になる |
| `crates/eml_vm/src/vm.rs`の専用のopの処理 (L1203-1360と、floatと比較のop) | `rows::`を呼ぶ | 1行の編集が約40 |
| `crates/eml_vm/src/tls.rs` (535行)、`gui.rs` (819行) | 移す。4つの関数を`GateHost`でgenericにする | import、約30行、traitの実装1つ |
| `crates/eml_vm/src/ffi.rs:21-69, 80-98`、`compile/mod.rs:166-220`、`compile/constants.rs:125-131` | sliceの上に書き直してeml_schedへ移す。誤りを写す | 移すのが約80行、変えるのが約10行 |
| eml_sched、eml_vm、eml_nrt、eml_native、eml_cliの`Cargo.toml` | feature | 小さい |
| jit-coreのファイル (N-18) | `eml_nrt/src/rows.rs` (一覧が増える)。eml_nativeの`unsupported.rs` (`StdRow`と`CRow`を消し、`Prim`を狭める)。`lower/consts.rs` (`EmptyMutArray`)。`abi.rs` (`c_signature`。ただ1つの`Linkage::Import`の関数がその結果も取る)。新しい`lower/c.rs`と`eml_nrt/src/c.rs` | 挙げたとおり |

## 7. テストとテストの変更の種類

- 新しいテスト:
  - eml_schedの`rows::tests`: D-9a (`LISTED`が`Extern::ALL`に等しく、どの行も1回ずつ)。D-9b (`faults`でない行で結果が`obj`のもの22行が、使い切った予算の下でも`Ok`を返す。単独で1秒以内)
  - eml_nativeの`tests/rows.rs`: D-9c (`Par`のほかのasyncでない行それぞれで、次のどちらか1つだけが成り立つ。エントリのない`inline`の行 (45)。記述子が行の種類に等しいエントリ`eml_x_{e:?}`がちょうど1つある (187)。`as`の識別子はどれも`eml_x_`と変種の名前。`eml_x_c_*`のほかに`eml_x_`のシンボルはない)。受け入れたUIのファイルが届かない行の族のagreeのテスト。`rows::extern_call_cost` (`#[ignore]`)
  - eml_nrt: Miriの`entries::x_*` (parkする行: メモリのHostで、ないファイルを`Std.FileIn.open`する。`faults`でない行: `StrConcat`。`float`の行: `FloatPow`。`unit`の引数: `FloatInfinity`。`FailIo`は`Raised`をparkする)。jit-rtのABIのテストに、新しい引数の種類の並びごとのechoのエントリ
  - eml_nativeの`tests/c.rs` (fixtureは`Linker::with_symbols`で渡す): `int32_t`に収まる値と収まらない値 (捕まえない。VMの文と地点)。負の`size_t`。void。`int32_t`の結果の-1。`F64`と`I64`が交互の9つの引数。`Text`。`UnboxedArray`。Cが書く`MutUnboxedArray`。handleの往復とdtorが1回だけ走ること (censusが綺麗)。nullのhandle。収まらない値を受けた`pure`のC行 (捕まえない)
  - 1つのCLIFのpin: 狭めの確かめ、呼び出し、handleのfaultの読み
- 種類(3): VMのテスト、UIのテスト、REPLのトランスクリプト、`bench.rs`の`RunStats`のスナップショットはすべて変わらない
- 種類(2): ネイティブのスナップショット2つ (`ui::run_native`、`ui::run_fail_native`)
- 種類(1): `-- native: skip`の印は1つずつ挙げ、「VMとの違い」の項目を名指す理由を付ける (出ない見込み)。jit-coreの、種の部分集合を固定するテスト (`rows::tests::the_listed_rows_are_the_seed_and_the_inline_rows`と、`rows::CALLED`か`Form::StdRow` / `Form::CRow`を見る`unsupported.rs`のテスト) は、D-9aとD-9cに置き換わる。書き直しで、それぞれを種類とともに挙げる
- どのテストも単独で1秒以内に終わる。Miriの時間はtesting.mdに記録する

## 8. 書くdocs/specの節

- spec/native.md: 「extern の行」(一覧とその節、4つの展開、シンボルの規則、wrapperの表、失敗しないことの不変条件、例外、費用のモデルと測った呼び出しの費用)。「C の関数の呼び出し」(5つの手順、ポインタの寿命、スタックの余裕)
- spec/vm.md: 「基本操作と extern」(armと専用のopが`rows::`を呼ぶ)。L480 (`GuiPark`はeml_schedにある)。L1080 (VMの変換は`ffi.rs`にあり、ネイティブは下ろしで同じ表に従う)
- spec/object-model.md: 「extern」(`run_extern`を名指しているなら)
- implementation/architecture.md: L99、L326。eml_schedの`rows`、`tls`、`gui`、`ffi`、`constants`。feature
- implementation/testing.md: `-p eml_sched`を含むfeatureの組み合わせ。L681。新しいテスト。Miriの時間
- implementation/status.md: asyncでないstdの行と、blockingでないC行はすべてネイティブで走る。`Std.Tls` / `Std.Gui`の行には、asyncの行が入って初めて届く (jit-tasks)
- CLAUDE.md: architectureの行 (eml_sched、eml_vm、eml_gui) と、clippyのfeatureの組み合わせの行
- future/native.mdの「extern のエントリ」とfuture/ffi.mdのL30、L86、L106、L125: 縮めるか、向け先を改める

## 9. 受け入れの条件

- `cargo nextest run`が通る。保留のスナップショットはネイティブのもの2つだけ。`cargo nextest run -p eml_vm --test integration bench::`は変わらない
- ネイティブで受け入れてVMと同じになるのが`run`で323以上、`run-fail`で42以上 (挙げた印の分を引く)。ヒストグラムは`Par`、`AsyncRow`、`BlockingC`、`Prim`、`Clause`、`Prompt`だけを持つ。どのテストも届かない行の一覧が空
- `cargo nextest run -p eml_sched -E 'test(rows::)'`、`cargo nextest run -p eml_nrt -E 'test(rows::) | test(c::)'`、`cargo nextest run -p eml_native -E 'test(rows::) | test(c::)'`が通る
- `cargo clippy --all-targets -- -D warnings`。`cargo clippy -p eml_sched -p eml_vm -p eml_io -p eml_nrt --all-targets --no-default-features -- -D warnings`と、それに`--features tls`を付けたもの、`--features gui`を付けたもの。`cargo build --features eml_cli/gui-native`。CLAUDE.mdにある`.#cross`のWindowsのclippyの2つのコマンド
- `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::) | test(gui_off::)'`が通る
- N-2のgrep。eml_schedの行は`--features tls,gui`でも流す
- `grep -rn 'thread_local' crates/eml_nrt/src crates/eml_native/src`が空
- `grep -rn 'eml_fatal' crates/eml_nrt/src/rows.rs crates/eml_nrt/src/c.rs`が空
- `grep -rn 'Linkage::Import' crates/eml_native/src`が1つの関数に当たる
- `git diff <段の最初のコミット> --stat -- crates/eml_rt`が空 (N-29は当たらない)
- `cargo nextest run --profile timing -j 1`が通る。Closeでbenchmarks.mdの「ネイティブの数値」を入れ替える (N-39)

## 10. 危険・代わりの手・未解決

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| X-1 | 334行の一覧で`macro_rules!`の誤りが読みにくい | 単位4、7 | 本体は短くし、長いものは名前の付いた関数を呼ぶ。constのassertが変種を名指す | 書いたとおり |
| X-2 | D-11のcrateをまたぐ呼び出しでVMが遅くなる | 単位5 | 前提で受け入れる (D-6)。既定で`#[inline]`。数値を記録する | 受け入れる |
| X-3 | `I32`のC ABIの拡張の規則が違う (Appleのarm64) | 単位9 | 明示の`ireduce` / `sextend`。fixtureを4つのターゲットすべてで流す | 書いたとおり |
| X-4 | emlの深さの上限で、Cが余裕より多くのスタックを使う | 単位9 | spec/native.mdに書く。Rustのエントリと同じくguard pageに当たる | 書いたとおり |
| X-5 | `NativeMachine`が`JITModule`を作る前に`symbol_lookup_fn`の名前の表を持てない、またはC関数ごとの名前のデータのシンボルをロードで埋められない (付録04のA-5) | 単位9 | 書き直しで確かめる | 呼ばれる側を名前ごとのデータのシンボルに入れ、`call_indirect`で呼ぶ (U-S3-1) |
| X-6 | ネイティブのハーネスが、fixtureを含むVMの`Linked`を両方のエンジンに渡さない (付録04のA-6) | 単位9 | 書き直しで確かめる | fixtureのファイルに`-- native: skip`を付ける (種類(1)) |
| X-7 | jit-rtの後も`bytes_empty`が失敗しうる (付録04のA-10) | 単位4 | D-9bが落ちる | 計画でその行を`faults: true`にする (種類(2)) |
| U-S3-1 | AOTが`eml_c$<name>`を結ぶ方法 | - | native-aot | JITでは`symbol_lookup_fn` |
| U-S3-2 | containerの行とCのポインタの引数のinline化 | - | native-inline | 今はエントリ (D-20) |
| U-S3-4 | `GateHost`か`WorkerCore`か | - | jit-tasks | jit-tasksまで`GateHost` |
| U-S3-6 | REPLの追加をまたぐCの表 | - | jit-repl | `CTables`はすでに名前で引く。トランスクリプトで確かめる |
| Q-1 | handleの型 (`hdesc`) と空の`MutArray`のデータのシンボルの名前 (C-6が名付けるのは`c:`だけで、付録04のD-8もhandleの型の接頭辞を仮の`h:`とする) | 単位7、9 | 書き直しで、C-6の方式の中で決める | 種類ごとに接頭辞を1つ持ち、中身だけで名付ける (たとえば`h:<handleの型の名前>`)。`DatumRow`がその種類を得る |

付録04は00の節Cを織り込み、00と同じことを書く: C-1 (inlineの行とその表のテストはjit-coreのもので、N-27の比べる相手もjit-coreから`rows::`である。`inline`の行は`as`の識別子を書かず、`Listed.entry`は`None`である。この段はVMに同じ関数を呼ばせるだけ)。C-2 (`eml$raise(ctx, function, site, purity)`と`worker::record(ctx, self, None)`)。C-3 (eml_nativeのテストは`rows`と`LISTED`にeml_nrtを通して届く)。C-4 (`Listed`はjit-coreから)。C-5 (`FATAL_RECORDED`は`enter.rs`にある)。C-6 (C関数のデータのシンボルは`c:<Module.f>`)。R-13 (下限)。付録04のU-S3-5 (`fail_io`の共有) はN-22で決まっている。
