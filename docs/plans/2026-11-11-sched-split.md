# 実行の方式によらない層をeml_vmから出す段 (sched-split)

- 作成日: 2026-11-11
- 基準: 前の段 (jit-rt) の`Close jit-rt`のコミット。この文書と付録の`ファイル:行`の参照はmain @ 432fffb4の状態を指す。基準とのずれは最初の作業S0で確かめる
- 状態: 判断はすべて決まっている (付録00)。Fableのadvisorによる実行できる計画の査読は済み、受け入れた助言はこの文書に入っている (付録00のU-11)

## 1. 目的と完了の条件

- 目的: crate `eml_sched` (multicore.mdの「層の構成」が取っておいた名前) を作り、ネイティブの実行系が共有する`eml_vm`の部分をファイルごとそこへ移す。移すのは、タスクのセルの核`cells.rs`とその`import`のテスト、実行の誤りの型 (`error.rs`)、`Park`、doorbell、時計と`DriverKind` (`driver.rs`)、`RunConfig`、`Finished`、`RunStats`、`Sched`、`Seat`である。VMだけの段で、ふるまいは変えず、`eml_vm`と`eml_cli`は今と同じ公開の名前を再エクスポートし続ける。開発だけのfeature `testing`が、`eml_vm`の単体テストの使うテストの道具を運ぶ (N-41)
- 完了の条件 (付録01の§5.3): S0〜S3とCloseをそれぞれ1コミットで入れること。テストとスナップショットがすべてバイト単位で変わらないこと (テストの変更は種類(3)だけ)。N-2のeml_schedの行とN-41の確かめが通ること。`cargo tree -p eml_sched`に`eml_vm`が出ないこと。各作業の完了の条件と9の全体の検証が、すべて期待どおりになること

## 2. 範囲

- 範囲に入るもの: 新しい`crates/eml_sched` (`Cargo.toml`、`src/{lib,cells,error,park,doorbell,driver,config}.rs`、`tests/{main,imports}.rs`)。`crates/eml_vm` (`Cargo.toml`、`src/lib.rs`、`src/sched.rs`、`src/workers.rs`、`src/jobs.rs`、`tests/tasks.rs`)。`crates/eml_test_support/src/lib.rs`と`crates/eml_cli/tests/ui.rs` (`RunStats`の5か所)。根の`Cargo.toml`。`.github/workflows/ci.yml`。`CLAUDE.md`。`docs/implementation/{architecture,testing,benchmarks}.md`。`docs/spec/vm.md`。`docs/future/multicore.md`のL63。`docs/future/roadmap.md` (Close)
- 範囲に入らないもの (括弧の中が受け持つ段): `eml_nrt`から`eml_sched`への辺 (jit-core。最初に使う所と一緒に)。`extern_rows!`と種の行の関数 (jit-core)。`eml_sched`の`tls`、`gui`、`gui-native`のfeatureと、`tls.rs`、`gui.rs` (jit-externs)。`Policy`、`Scheduler`、`Memory`、`Normal`、`Sequential`、`Seeded`、`TaskId`、`workers.rs`、`monitor.rs`、`jobs.rs`、`counters.rs`、`RunCounts`、`WORKER_STACK`、`execute`、`Machine`、`Exec`のtrait、feature `testing`を消すこと (jit-tasks)。`VmFault`から`RunFault`への名前の変更 (jit-core)

## 3. 背景

- `error.rs`は、1つの`From`の実装のために`cells::CellFault` (2つの腕のenum。`cells.rs:188`) をimportする。`park.rs`がimportするのは`eml_rt::IoWaker`だけである。`driver.rs`は`eml_io`、`VmFault`、`park::PollSource`、`sched::NO_DRIVER`をimportする [付録01の§7の11]。だからファイルはまるごと、S1 → S2 → S3の順で移す (付録02のD-20)
- `RunConfig`は`Sched`、`Clock`、`Doorbell`、`VirtualSignal`、`Seat`、`ParkFactory`を要し、`Finished`は`RunStats`を、`RunClock::now`は`NO_DRIVER` (`eml_vm/src/sched.rs:286-287`) を要する
- `eml_vm`の単体テストは`cells.rs`の`#[cfg(test)]`の道具を54回使う。feature `testing`がない状態で移すと、コンパイルの誤りが54出た。`cells.rs`にただ1つある`cfg!(test)`の式 (`Registry::host_bin`) のため、道具を移すと`jobs::tests`の5つが落ちた ("a host has no bin for its handles")。道具`settled`に準備の1行を足すと直る [付録02の§7の2]
- `RunStats`は`#[non_exhaustive]`で、移した後は`eml_vm`が構造体の式で作るので`E0639`になる。clippyの`field_reassign_with_default`が5か所で出る
- 移すファイルの`pub(crate)`の数: cellsが247、driverが11、parkが2、errorが1
- 試し (432fffb4の上にS1+S2+S3の最後の状態を作ったもの。aarch64 macOS) では、`cargo nextest run` → `4878 tests run: 4878 passed, 22 skipped`、ワークスペースとfeatureの組み合わせのclippyは警告なし、`cargo tree -p eml_sched`は`eml_io`、`eml_rt`、`libc`を示し、N-41の2つの数はどちらも0だった。3つのコミットを1つずつビルドしてはいない (付録02のA-5)
- 付録 (英語) は`docs/plans/2026-11-11-jit/`にある。この段の設計は付録02の§1.2 (D-20〜D-26)、作業は付録02の§5.2である

## 4. 前提

| 要るもの | 使う所 | 確かめ方 → 期待 | ないとき |
|---|---|---|---|
| NixのdevShell (cargo-nextest) | すべての作業 | `cargo nextest --version` → 版を表示する | 止めて報告する |
| `.#cross`のshell | S3、9の全体の検証 | `nix develop .#cross -c cargo check -p eml_rt --target x86_64-pc-windows-msvc` → `Finished` | 止めて報告する |
| `.#miri`のshell | S1、9の全体の検証 | `nix develop .#miri -c cargo miri --version` → 版を表示する | 止めて報告する |
| PLaMoの翻訳 (`plamo-translate`のMCPかCLI) | すべての作業の日本語の文 | `plamo-translate --help` → 終了コード0 | `natural-japanese:natural-japanese`のskillに従う (CLAUDE.md) |
| macOS (`bench/*.sh`はmacOSでだけ動く) | Close | `uname` → `Darwin` | S0〜S3はどのOSでも行う。macOSがないときは、表の入れ替えをCloseの直後の別のコミット`Measure sched-split`にする (ロードマップの「進め方」) |
| `gh`と、pushへの利用者の許し | マージの前にCIを流したいと利用者が言ったときだけ | `gh auth status` → ログインしている | マージの後の普段のCIで確かめる |

## 5. 決定

### 5.1 長期の方針 (変えるには見直しが要る)

実装がこの節と食い違ったら、止めて報告する。全文は付録01にある。

#### D-1: `eml_sched`は実行の方式によらない層で、2回に分けて育つ
- 決定: `eml_sched`は、VMとネイティブの実行系が共有するものを持つ。この段はタスクのセルと実行のインタフェースを移し、jit-tasksがworkerの表、受け箱、monitor、jobを`Exec`のtraitの後ろに移す。`eml_sched`はCranelift、`eml_lir`、`eml_vm`、`eml_nrt`、`eml_native`に依存しない。`RuntimeError`とその`Display`は`eml_sched`で1度だけ定める
- 理由: 実行の誤りの型はjit-coreが要る。jit-externsの、タスクの操作でない`Std.Tls`と`Std.Gui`の腕は、`cells`の型と`DriverKind`をimportする。ネイティブのコードが頼る前に`cells.rs`を1度移せば、どちらの順の問題もなくなる
- 採らなかった案: 誤りの型だけを持つ`eml_sched`をjit-coreの中で作る案 (`error.rs`が`cells::CellFault`に依存するので、コード生成の段にcellsの作り直しを持ち込む)。`RunStats`を`eml_vm`に残す案 (`Finished`がそれを含む)
- 及ぶ範囲: `eml_vm`、`eml_cli`、`eml_test_support`。後で`eml_nrt`
- 見直す条件: 移したものが`eml_vm`の何かを要すると分かったとき
- 適合の確かめ: N-1。N-2のeml_schedの行 (`test "$(cargo tree -p eml_sched -e normal --target all | grep -c -E 'cranelift|eml_lir|eml_vm|eml_native|eml_nrt')" -eq 0`)。N-23 (`grep -rn 'for RuntimeError' crates/*/src | grep Display` → 1行)
- リポジトリに残す所: architecture.md (S1〜S3)、multicore.mdのL63 (S1)

#### D-2: 開発だけのfeature `testing`
- 決定: `eml_sched/testing`が切り替えるのは、テストの道具とテストのフックだけである (項目だけで、テストでない論理に`cfg!(feature = "testing")`の式を置かない)。有効にするのはdev-dependenciesだけで、それを使う単体テストを持つファイルを移す段jit-tasksが消す。そのときまだ道具が要るテストは、その段で公開のAPIの上に書き直す。jitの段は、ほかに新しいcargoのfeatureを作らない
- 理由: `eml_vm`の54の道具の使用を公開のAPIで書き直すと、移すだけの段がテストの書き直しになる。CLAUDE.mdは既存のテストを変えないためのフラグを禁じるが、このfeatureは期待する値ではなくテストの道具だけを切り替え、sched-splitからjit-tasksまでだけあってjit-tasksが消す。利用者はこれをその規則の例外として認めた (付録00のU-12)。査読でこれを蒸し返さない
- 及ぶ範囲: `eml_sched`、`eml_vm`のdev-dependencies
- 見直す条件: テストでないビルドが道具を要するとき
- 適合の確かめ: N-41 (`grep -rn 'feature = "testing"' crates/eml_sched/src | grep -c 'cfg!'` → `0`。`cargo tree -p eml_cli -e normal,build -f '{p} {f}' | grep -c 'eml_sched.*testing'` → `0`。後者はCIの依存の辺の手順でも確かめる)
- リポジトリに残す所: architecture.mdの「crate の依存関係」(S1)

#### D-3: VMだけの段は期待する値をすべて保つ
- 決定: この段はふるまいを変えない。テストとスナップショットはすべてバイト単位で同じで、テストの変更はどれも種類(3)である。ネイティブの速さを先にする方針 (付録00のU-4) は、ネイティブのものを作らないのでここでは当たらない。きれいさを先にする方針 (U-5) は当たる: ファイルはまるごと移し、間に挟む層も二重の定義も置かない
- 適合の確かめ: 9の指紋とバイナリごとのテストの数
- リポジトリに残す所: なし (Closeのコミットのメッセージ)

### 5.2 短中期の設計

細部は付録02の§1.2にある (そこでのD-20〜D-26を、この計画ではD-10〜D-16と呼ぶ。括弧に元の番号を書く)。

- D-10 (付録02のD-20): `crates/eml_sched`は`[lib] doctest = false`、`autotests = false`、`[[test]] integration`を持ち、`eml_io`、`eml_rt`、`cfg(unix)`の下の`libc`に依存し、`[features] testing = []`を持つ。モジュールは`cells`、`config`、`doorbell`、`driver`、`error`、`park`である。`config.rs`は新しく、`RunStats`、`Finished`、`RunConfig` (と`Default`、`with_*`)、`Sched` (と`seed`)、`Seat`を持つ。`NO_DRIVER`は`driver.rs`へ移る。この段では`tls`や`gui`のfeatureを置かない
- D-11 (D-21): `eml_vm`は今の道を保つ。`lib.rs`に`pub(crate) use eml_sched::{cells, doorbell, driver, error, park};`と`pub use eml_sched::config::{Finished, RunConfig, RunStats};`、`sched.rs`に`pub use eml_sched::config::Sched;`と`pub(crate) use eml_sched::driver::NO_DRIVER;`、`workers.rs`に`pub use eml_sched::config::Seat;`を置く。`eml_cli`と`eml_test_support`のソースは、D-14の所のほかは変えない
- D-12 (D-22): 移したファイルの`pub(crate)`はすべて`pub`にする。ほかの可視性は変えない
- D-13 (D-23): `cells.rs`の25の`#[cfg(test)]`を`#[cfg(any(test, feature = "testing"))]`にする (項目は付録02のD-23にある)。`crates/eml_vm/Cargo.toml`の`[dev-dependencies]`に`eml_sched = { workspace = true, features = ["testing"] }`を足す。`cfg!(test)`の式は1つ残す (featureに広げると、統合テストとUIテストの下のライブラリがreleaseのビルドより甘くなる)。`crates/eml_vm/src/jobs.rs`の道具`settled`には、`let mut driver = TestDriver::new(None);`の後に`registry.lock().set_bin(0, Arc::new(cells::HostBin::new(driver.bin(), false)));`を足す。`cells.rs`の2つの`cfg_attr(not(feature = "gui"), ..)`は消す
- D-14 (D-24): `RunStats`から`#[non_exhaustive]`を外す (`RunConfig`には残す)。「既定の値を作ってから代入する」5か所を、同じ欄と値の構造体の更新の式にする。`crates/eml_test_support/src/lib.rs`の`channel_independent` (L317-323)、`driver_independent` (L331-339)、`crates/eml_cli/tests/ui.rs`のL532-535、L575-578、L612-616である
- D-15 (D-25): `the_task_cells_import_only_std_and_the_runtime`を`crates/eml_vm/tests/tasks.rs` (L276-293) から`crates/eml_sched/tests/imports.rs`へ、同じ名前と本体 (`include_str!("../src/cells.rs")`) のまま移す
- D-16 (D-26): `cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write counters::`を、`cargo miri test -p eml_sched --lib -- cells::a_read cells::a_write`と`cargo miri test -p eml_vm --lib -- counters::`に分ける (ci.yml L238-241、CLAUDE.md L51、testing.md L197、L202、L706、L787)。TSanの`-p eml_vm --lib counters::`は変えない。clippyに`cargo clippy -p eml_sched --all-targets -- -D warnings`の行を足す
- D-17: 費用のモデル (N-38)。この段はネイティブの道を作らない。VMの行が動きうる仕組みは、crateをまたぐ`cells`の関数のinline化だけで、Closeはそうした動きを記録し、ゲートにはしない。諦めた最適化はない

## 6. 実装の制約と約束

- コメントと文書は日本語で書く。英語で下書きしてPLaMoで訳し (`form: plain`、識別子は用語集に入れる)、見直す。日本語とLatin文字やインラインコードの間に空白を入れない (CLAUDE.md)。移すファイルのコメントはそのままにし、変わったcrateの道を名指すコメントだけを書き直す
- コードと`docs/`は、`docs/plans/`もこの計画のIDも引かない。ない見出しを引くと`citations::every_cited_heading_exists`が落ちる
- 移したファイルを名指す文書は、それを移すコミットで直す
- テストの変更は種類(3)だけで、すべてを挙げる: (a) `the_task_cells_import_only_std_and_the_runtime`を本体を変えずに`crates/eml_sched/tests/imports.rs`へ移す。(b) `cells.rs`の82の単体テストがファイルと一緒に移る。(c) `crates/eml_vm/src/jobs.rs`の道具`settled`に準備の1行を足す (期待する値は変わらない)。(d) `driver.rs` (1)、`park.rs` (2)、`doorbell.rs` (3) の6つの単体テストがファイルと一緒に移る。(e) D-14の5か所 (同じ欄と値)。種類(1)と(2)はない
- `eml_rt`は変えない (eml_rtのゲートはない)
- きれいさを先にする (付録00のU-5): 互換のための挟み物も、二重の型も置かない。`eml_vm`は再エクスポートする
- 行の番号は手がかりで、名指した項目が決める。付録の行の番号は432fffb4のもので、jit-rtがci.yml、CLAUDE.md、testing.md、architecture.mdの行をずらしている
- `Close jit-rt`から作ったブランチ`plan/sched-split`で作業する。この順で1つのコミットが単独でコンパイルできなければ (付録02のA-5)、S1+S2かS2+S3をまとめ、コミットのメッセージにそう書く

## 7. 実装の作業

共通のゲートG (`git add -A`の後にリポジトリの根で流す):
1. `cargo nextest run` → `0 failed`
2. `cargo clippy --all-targets -- -D warnings` → `error`も`warning`も出ない
3. `cargo fmt --check` → 出力なし
4. 指紋`git ls-files 'crates/*/tests/**/*.snap' 'tests/ui' 'crates/eml_vm/tests/bench.rs' | xargs shasum | shasum` → S0で記録した基準と同じ

| ID | 作業 | 元の決定 | 前の作業 | 並行 |
|---|---|---|---|---|
| S0 | 段の始めの確かめ (`Start sched-split`) | - | `Close jit-rt` | 不可 |
| S1 | `Move the task cells and the run errors to eml_sched` | D-1、D-2、D-10〜D-13、D-15、D-16 | S0 | 不可 |
| S2 | `Move the clock, the doorbell and Park to eml_sched` | D-10、D-11 | S1 | 不可 |
| S3 | `Move RunConfig and Finished to eml_sched` | D-10、D-11、D-14 | S2 | 不可 |
| Close | `Close sched-split` | すべて | S3 | 不可 |

各作業の後のバイナリごとのテストの数 (`cargo nextest list -p eml_vm -p eml_sched --message-format oneline 2>/dev/null | awk '{print $1}' | sort | uniq -c`。各行は`<binary-id> <test>`である):

| バイナリ | 基準 | S1の後 | S2の後 | S3の後 |
|---|---:|---:|---:|---:|
| `eml_vm` (lib) | 201 | 119 | 113 | 113 |
| `eml_vm::integration` | 579 | 578 | 578 | 578 |
| `eml_sched` (lib) | - | 82 | 88 | 88 |
| `eml_sched::integration` | - | 1 | 1 | 1 |
| 和 | 780 | 780 | 780 | 780 |

(基準と最後の列は試しで測った。S1とS2の列は、ファイルごとのテストの数 (`cells.rs` 82、`doorbell.rs` 3、`park.rs` 2、`driver.rs` 1、`error.rs` 0) から求めた。jit-rtが`eml_vm`のテストの数を変えていたら、どの列も同じだけずらす。)

### S0: 段の始めの確かめ
- 目的: 段の最初のコミット (ロードマップの「進め方」)。この計画はjit-rtの計画と一緒に入っているので、S0は基準に照らして確かめ、基準の値をこの計画に記録する (jit-coreのK0、jit-applyのA0と同じ形)
- 対象: `docs/plans/2026-11-11-sched-split.md`だけ
- 作業:
  1. `Close jit-rt`から`git switch -c plan/sched-split`
  2. Gの1〜3を流す
  3. `cargo nextest run`の合計を、`Close jit-rt`のコミットのメッセージに書いた合計と比べる
  4. バイナリごとの数のコマンドが`eml_vm`の行に`201`、`eml_vm::integration`の行に`579`を出すことを確かめる (コマンドの形がS1〜S3で頼れるかをここで確かめる)
  5. 指紋を記録する
  6. 付録02の§1.2と§5.2の参照 (`eml_vm/src/lib.rs`のL45-56、L62-176、L190-285。`sched.rs`のL101-121、L286-287。`workers.rs`のL206-213。`tests/tasks.rs`のL276-293。`eml_test_support/src/lib.rs`のL317-339。`eml_cli/tests/ui.rs`のL532-616。ci.ymlのL238-241) の今の場所を探す
  7. 3〜6の結果をこの計画の9節に書き、`Start sched-split`としてコミットする
- 実装者に任せること: なし
- 完了の条件:
  - Gの1〜3が通る。合計が`Close jit-rt`のメッセージの合計と同じ
  - バイナリごとの数が`201`と`579` (違えば、ずれを9節に記録し、7の表のどの列も同じだけずらす。コマンドが数を出さなければ止めて報告する)
  - `git diff --name-only HEAD~1 HEAD` → `docs/plans/2026-11-11-sched-split.md`だけ

### S1: `Move the task cells and the run errors to eml_sched`
- 目的: D-1、D-2 (付録02のD-20〜D-23、D-25、D-26)
- 対象: 根の`Cargo.toml`。`crates/eml_sched/{Cargo.toml, src/lib.rs, src/cells.rs, src/error.rs, tests/main.rs, tests/imports.rs}`。`crates/eml_vm/{Cargo.toml, src/lib.rs, src/jobs.rs, tests/tasks.rs}`。`ci.yml`。`CLAUDE.md`。下に書く文書
- 作業 (付録02の§5.2のS1の順):
  1. 根の`Cargo.toml`に`eml_sched = { path = "crates/eml_sched" }`。`crates/eml_sched/Cargo.toml`はD-10のとおりで、このコミットの依存は`eml_rt`だけにする。`crates/eml_vm/Cargo.toml`の`[dependencies]`に`eml_sched.workspace = true`、`[dev-dependencies]`に`eml_sched = { workspace = true, features = ["testing"] }`
  2. `git mv crates/eml_vm/src/cells.rs crates/eml_vm/src/error.rs crates/eml_sched/src/`。`crates/eml_sched/src/lib.rs`にcrateのdocと`pub mod cells; pub mod error;`
  3. 2つのファイルで`sed -i 's/pub(crate)/pub/g'`。`error.rs`は`use crate::cells::CellFault;`を残す
  4. `cells.rs`: 2つの`cfg_attr(not(feature = "gui"), ...)`を消し、D-13の属性の変更をコンパイラに従って当てる (`cargo check -p eml_sched`と`cargo check -p eml_sched --features testing`が通り、次に`cargo test -p eml_vm --lib --no-run`が通る)。`cfg!(test)`は残す
  5. `crates/eml_vm/src/lib.rs`: `mod cells;`と`mod error;`を消し、`pub(crate) use eml_sched::{cells, error};`を足す。`crates/eml_vm/src/jobs.rs`: D-13の道具の1行
  6. `import`のテストを移す (D-15): `crates/eml_sched/tests/main.rs` (`mod imports;`) と`tests/imports.rs`
  7. `ci.yml`、`CLAUDE.md`、testing.mdのコマンド (D-16)。CIの依存の辺の手順に`test "$(cargo tree -p eml_sched -e normal --target all | grep -c -E 'cranelift|eml_lir|eml_vm|eml_native|eml_nrt')" -eq 0`と`test "$(cargo tree -p eml_cli -e normal,build -f '{p} {f}' | grep -c 'eml_sched.*testing')" -eq 0`を足す。clippyの行`cargo clippy -p eml_sched --all-targets -- -D warnings`
  - 同じコミットの文書: architecture.md (リポジトリの一覧。crateの表の`eml_sched`の行: エンジンが共有するタスクのセルの核と実行のインタフェースで、`eml_rt`と`eml_io`に依存し、Cranelift、`eml_lir`、`eml_vm`、`eml_nrt`、`eml_native`に依存しない。「crate の依存関係」: `eml_vm`から`eml_sched`への辺、開発だけのfeature `testing`、それが切り替えるもの、jit-tasksが消すこと。`cells.rs`を挙げるL326)。vm.mdのL719 (`cells.rs`は`eml_sched`にある)、L792、L905、L958 (crateを書いている所) と、移した型の「置き場所」。testing.mdのL197、L202、L284-285、L706、L787。multicore.mdのL63 (`eml_sched`はセルと実行のインタフェースを持ち、スケジューラとworkerの表はまだ`eml_vm`にある)
- 実装者に任せること: 文書の日本語の言い回し。コンパイラに従って属性を変える順
- テストの変更の種類: (3)。6の(a)、(b)、(c)
- 完了の条件:
  - G。「S1の後」の列
  - `grep -c 'cfg(any(test, feature = "testing"))' crates/eml_sched/src/cells.rs` → `25`。`grep -c 'cfg!(test)' crates/eml_sched/src/cells.rs` → `1`。`grep -rn 'feature = "testing"' crates/eml_sched/src | grep -c 'cfg!'` → `0`。`grep -c 'feature = "gui"' crates/eml_sched/src/cells.rs` → `0`
  - `cargo tree -p eml_cli -e normal,build -f '{p} {f}' | grep -c 'eml_sched.*testing'` → `0`
  - `test "$(cargo tree -p eml_sched -e normal --target all | grep -c -E 'cranelift|eml_lir|eml_vm|eml_native|eml_nrt')" -eq 0` → 終了コード0
  - `cargo clippy -p eml_sched --all-targets -- -D warnings`と、`--features testing`を付けた同じコマンド → 出力なし
  - `cargo miri test -p eml_sched --lib -- cells::a_read cells::a_write`の2つのMiriの実行 (`nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance"`と、それに`-Zmiri-tree-borrows`を足したもの) → `test result: ok`
- 落とし穴: featureを作る前は、`eml_vm`の単体テストでコンパイルの誤りが54出る。道具の1行がないと、`jobs::tests`の5つが"a host has no bin for its handles"で落ちる。`cfg!(test)`をfeatureに広げない

### S2: `Move the clock, the doorbell and Park to eml_sched`
- 目的: D-1 (付録02のD-20、D-21)
- 対象: `crates/eml_sched/{Cargo.toml, src/lib.rs, src/park.rs, src/doorbell.rs, src/driver.rs}`。`crates/eml_vm/src/{lib.rs, sched.rs}`。architecture.md。vm.mdの「時計と driver」と「メインのworkerの待ち」のファイルを名指す所
- 作業: `park.rs`、`doorbell.rs`、`driver.rs`を`git mv`し、`pub(crate)`を`pub`にする。`eml_sched/Cargo.toml`に`eml_io.workspace = true`と`[target.'cfg(unix)'.dependencies] libc.workspace = true`を足す。`driver.rs`では`use crate::VmFault;`を`use crate::error::VmFault;`に替え、`use crate::sched::NO_DRIVER;`を消し、その定数とdocコメント (`eml_vm/src/sched.rs` L286-287) を`driver.rs`の`#[cfg(test)] mod tests`の前に置く。`eml_vm/src/sched.rs`に`pub(crate) use eml_sched::driver::NO_DRIVER;`を足す。`eml_vm/src/lib.rs`の`pub(crate) use eml_sched::{...}`の行に3つの名前を足す。`eml_sched/src/lib.rs`に3つの`pub mod`を足す
- 実装者に任せること: 文書の日本語の言い回し
- テストの変更の種類: (3)。6の(d)
- 完了の条件: G。「S2の後」の列
- 落とし穴: 定数をテストのモジュールの後に置くと、clippyの`items_after_test_module`に当たる

### S3: `Move RunConfig and Finished to eml_sched`
- 目的: D-1 (付録02のD-20、D-21、D-24)
- 対象: 新しい`crates/eml_sched/src/config.rs`。`crates/eml_vm/src/{lib.rs, sched.rs, workers.rs}`。D-14の5か所。vm.mdの「置き場所」とL410。architecture.mdの「CLI と lib API」(`eml_vm`を定める所と書いていれば)
- 作業: `config.rs`に、変えずに移す: `RunStats`と`fields()` (`#[non_exhaustive]`は外す)、`Finished`、`RunConfig`と`Default`と`with_*` (`eml_vm/src/lib.rs` L62-176、L190-285)、`Sched`と`seed()` (`sched.rs` L101-121)、`Seat` (`workers.rs` L206-213)。importは`std::sync::Arc`、`std::time::Duration`、`eml_rt::{Control, Limits}`、`crate::doorbell::Doorbell`、`crate::driver::{Clock, JOB_QUIESCE_LIMIT, VirtualSignal}`、`crate::park::ParkFactory`。`eml_vm`にD-11の再エクスポートの行 (コンパイラが使わないと言うimportは消す)。D-14の5か所。文書: vm.md (選ぶためのenum `Sched`と実行の設定は`eml_sched`にあり、`Policy`と`Scheduler`は`eml_vm`の`sched.rs`に残る)
- 実装者に任せること: 文書の日本語の言い回し
- テストの変更の種類: (3)。6の(e)
- 完了の条件:
  - G。「S3の後」の列。ワークスペースの合計がS0と同じ
  - `grep -c 'pub(crate)' crates/eml_sched/src/*.rs` → どの数も`0`
  - `grep -rn 'for RuntimeError' crates/*/src | grep Display` → 1行で、`crates/eml_sched/src/error.rs`
  - `git diff <S0の基準> --stat -- crates/eml_cli/src crates/eml_test_support/src`が`eml_test_support/src/lib.rs`だけを挙げる (付録02は`432fffb4`と書くが、それはsched-splitを先にした場合で、ここでは`Close jit-rt`と比べる)
  - `cargo clippy -p eml_cli --no-default-features --features run -- -D warnings`、`--features run,gui`での同じコマンド、`cargo clippy -p eml_io -p eml_vm --all-targets -- -D warnings` → 出力なし
  - `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings`と、`--target x86_64-unknown-linux-gnu`での同じコマンド → 出力なし (移したファイルには`cfg(unix)`と`cfg(not(unix))`の半分があり、`eml_sched`は`libc`を`cfg(unix)`でだけ持つので、マージの前にWindowsとLinuxの型を確かめる)

### Close: `Close sched-split`
- 目的: 段を閉じる (ロードマップの「進め方」)
- 作業:
  1. spec: `grep -nE 'jit-|sched-split' docs/spec/vm.md` → 出力なし
  2. bench (macOS): `crates/eml_vm`を変えたので、まず`bench/scripts.sh`を流し (範囲を外れた行があれば別のコミット`Resize bench rows`で大きさを変える)、次にきれいな作業ツリーで`bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh`を流して、benchmarks.mdの「今の数値」の3つの表を入れ替える。ネイティブの道はないので、動いたVMの行は記録し、ゲートにはしない
  3. status.md: 変えない
  4. architecture.md: 確かめる
  5. roadmap.md: `sched-split`の項目を消し、`jit-core`の前提を直し、L337に最初の段階が済んだと書く
  6. overviewとREADME: crateを挙げているときだけ
  7. `CLAUDE.md`: Architectureの塊の`eml_sched`の行、Commands (分けたMiriの行、clippyの行)
  8. 5のD-nがどれも`docs/`にあることを確かめてから、`docs/plans/2026-11-11-sched-split.md`を消す (このファイルだけ)
  9. コミットのメッセージ: 移したものの一覧。「種類(3)だけ」と(a)〜(e)。feature `testing`とその終わり (jit-tasks)。`#[non_exhaustive]`を外した`RunStats`。bench
- 実装者に任せること: コミットのメッセージの言い回し
- 完了の条件:
  - G
  - `cargo nextest run -p eml_cli --test integration citations::` → `0 failed`
  - `ls docs/plans | grep -c 'sched-split'` → `0`
  - `grep -c '^- `sched-split`' docs/future/roadmap.md` → `0`
- この段はABIに関わるコードに触らない。CIはマージの後の普段の実行で確かめる (利用者が許せば、jit-rtのチェックポイントの手順を`plan/sched-split`で流してもよい)

## 8. 実行の順

S0 → S1 → S2 → S3 → Closeの順に1つずつ行い、どれも1コミットにする。順はimportで決まる。`driver.rs`は`error.rs`を、`config.rs`は`driver.rs`、`doorbell.rs`、`park.rs`を要する。sched-splitはjit-rtに依存しない (付録01の§1.5) が、この計画ではjit-rtの後に行う。

## 9. 検証の計画

- 基準の確かめ (S0、`Close jit-rt`で): Gの1〜3が通る。`cargo nextest run`の合計 (`Close jit-rt`のメッセージの合計)。バイナリごとの数`201`と`579`。指紋。S0がここに値を書く
- 全体の検証 (Closeの後): G。S1〜S3の完了の条件をHEADで流し直す。`cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::) | test(gui_off::)'` → `0 failed`。`cargo clippy -p eml_cli --no-default-features --features types -- -D warnings`と`--features core` → 終了コード0。`nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --lib -- counters::` → `test result: ok`。S3の2つのcrossのclippy → 出力なし。`nix build`が成功し、`cargo tree -p eml_cli -e normal,build -f '{p} {f}' | grep -c 'eml_sched.*testing'` → `0`
- 利用者が手で確かめること: architecture.mdの`eml_sched`の行と`testing`の段落を読む。Closeのコミットのbenchの表に、思わぬVMの変化がないことを見る

## 10. 危険と未解決の項目

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | S1 → S2 → S3の順で、あるコミットが単独でコンパイルできない (付録02のA-5) | S1〜S3 | 起きたとき | S1+S2かS2+S3をまとめ、コミットのメッセージにそう書く |
| Q-2 | resolver 3が`cargo build -p eml_cli`や`nix build`で`eml_sched/testing`を有効にする (A-4) | S1 | S1とCIのN-41の確かめ | 確かめが落ちる。featureはバイナリに入ってはならないので、止めて報告する |
| Q-3 | VMのbenchの行が動く (crateをまたぐ`cells`の関数のinline化) | Close | Closeで記録する | 記録し、ゲートにしない |
| Q-4 | jit-rtが基準のテストの数か行の番号を変えた | S0 | S0 | ずれを記録し、期待する数をそれだけずらす |
| Q-6 | WindowsかLinuxのビルドが移したファイルの`cfg`で壊れる | S3 | S3のcrossのclippy | 直してからマージする |
| Q-5 | `eml_sched/testing`を消すこと (付録02のU-7) | - | jit-tasks | jit-tasksがまるごと消す |

## 付録: 査読と照合で大きく変わった所

- 前の契約は、誤りの型だけを持つ`eml_sched`をjit-coreの中で作っていた。`error.rs`が`cells::CellFault`に依存するので、移すことはVMだけの独立した段になり、ロードマップの「段の列」に載せた (付録00のU-7)
- 試しで54の道具の使用が見つかった。それらを書き直す案は退け、N-41で縛りjit-tasksが消す開発だけのfeature `testing`にした
- `cfg!(test)`はfeatureに広げずにその場に残し、`jobs::tests`の5つは道具からbinを得るようにした
- `RunConfig`が要するので、`doorbell.rs`と`Seat`もこの段で移す (R-14)
- 各コミットが単独でコンパイルでき通るように、段を3つのコミットに分けた
- Fableのadvisorの査読 (U-11) を受けて、次を足した。S3と9にcrossのclippy。バイナリごとの数のコマンドに`--message-format oneline`。S0をコミットにし、jit-rtのCloseが記録する合計と比べる。feature `testing`を利用者が規則の例外として認めたこと (U-12)。行の番号は手がかりとする
