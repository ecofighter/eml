# platform-ci: macOS、Linux、Windows の CI

- 作成: 2026-10-23
- 基点: main @ edeac32d (「背景」と、すべての `file:line` はこの状態を指す)
- 状態: 計画は確定。未決の項目は既定つきで 10 にある
- 付録: [2026-10-23-platform-ci/](2026-10-23-platform-ci/)。試作の patch 3つと、別の target で clippy を流す道具を置く (下の「付録の使い方」)

## 1. 目的と完了の条件

- 目的: 今のテストを macOS、Linux、Windows の GitHub Actions で通す。Windows でビルドとテストを通し、toolchain を `rust-toolchain.toml` で 1.98.1 に固定する。Windows の Ctrl-C をコンソールの handler で受ける。後の段 (`io-driver`、`worker-threads`、`native-aot`、`net-windows`) はこの CI を前提にする
- 完了の条件:
  - 9 の全体の確認のコマンドが、開発機 (macOS) で期待どおりになる
  - draft PR で、CI の7つの check (`test (ubuntu-24.04)`、`test (macos-15)`、`test (windows-2025)`、`fmt`、`verus`、`miri (stacked borrows)`、`miri (tree borrows)`) がすべて通る (T-13)
  - 段を閉じるコミット (T-14) が入っている

## 2. 範囲

- 範囲に入るもの:
  - Windows の target は `x86_64-pc-windows-msvc` だけにする (D-7)
  - Windows で動かないテストの扱いと印 (D-1、D-2)
  - 改行を変えない `.gitattributes` (D-3)
  - Windows で違う動きをする所の移植: ディレクトリ、入口のパスの `\`、物理メモリ、文字コードの合わないテスト入力 (D-12〜D-15)
  - Ctrl-C の受け口の分割と、Windows のコンソールの handler (D-6、D-10、D-11)
  - `rust-toolchain.toml` と flake (D-4)、CI のワークフロー (D-5、D-17)、引用の検査の範囲 (D-18)
- 範囲の外のもの:
  - `TERMINATE` のビットと、`CTRL_CLOSE_EVENT` / `CTRL_SHUTDOWN_EVENT` の扱い (非同期とマルチコアの段)
  - std を通さない Windows のコンソールの読み手 (10 の Q-1)
  - Windows で子のプロセスを使う、移植できるテストと、`inherited_stdout` の移植 (`io-driver` の段。Q-2)
  - release のビルド、`nix build` の CI、成果物の配布、bench の CI (Q-5)

## 3. 背景

- CI はない。`.github/`、`.gitattributes`、`rust-toolchain.toml` はない。GitHub の `ecofighter/eml` は public で、Actions は有効、ワークフローは0本、既定の branch は `main` である (`gh api repos/ecofighter/eml/actions/workflows` → `total_count: 0`)
- Linux と Windows でテストを流した形跡はない。flake の systems は `x86_64-linux` と `aarch64-darwin` だけで (`flake.nix`)、x86_64-linux の Verus の derivation は評価だけされ、ビルドされたことがない (commit 6c6aafac のメッセージ)
- 基点で、Windows の target `x86_64-pc-windows-msvc` の `cargo check --workspace --all-targets` は通る。clippy の `-D warnings` は `crates/eml_rt/src/heap/tests/script.rs:77` の `unused_mut` の1つだけで落ちる。Linux の target の clippy は通る。どれも link はしていない
- `#[cfg(unix)]` で丸ごと外したテストは14ある (`eml_cli/tests/cli.rs` 6、`terminal.rs` の `interrupts` 3、`eml_rt/src/heap/tests/host.rs` 3、`eml_vm/src/hosts.rs` 2)
- UI テストのうち14本は、`CaptureHost` を通して本物の `sh`、`cat`、`sort`、`sleep`、`printf`、`./run.sh` を起こす (T-5 の一覧)
- CR のバイトを含む fixture が8つある。`.gitattributes` がないので、`core.autocrlf=true` の Windows で clone すると 2037 のファイルに CR が入る
- Ctrl-C: `crates/eml_cli/src/sigint.rs` が unix だけで `sigaction` を入れる (`SA_RESTART` なし)。`eml run` の2回目は `_exit(130)`。呼ぶ所は `main.rs:68-70` (`#[cfg(unix)]`) と `terminal.rs:31` である。`Control` は `crates/eml_rt/src/control.rs` にあり、`INTERRUPT = 1`、`MEMORY = 2`、Relaxed で読み書きする。`eml_rt` は signal の handler を入れない (`crates/eml_rt/CLAUDE.md`)
- Windows で今と違う動きをする std の行と host:
  - `physical_memory` は unix 以外で `None` を返す (`crates/eml_cli/src/lib.rs`)。`api.rs` の2つの assert は macOS と Linux のときだけ確かめる
  - `inherited_stdout` は `Unsupported` を返すので、`run (merge_stderr c)` が失敗する (`crates/eml_rt/src/host.rs`)
  - Windows の `File::open` はディレクトリを開けない。std が `FILE_FLAG_BACKUP_SEMANTICS` を付けないためである (rust-src 1.98.1 `sys/fs/windows.rs`)。unix では開けて、読みで失敗する
  - 入口の名前は `/` で区切る (`eml_hir` の load.rs:135、repl.rs:153、:334)。Windows では `exact_spelling` の `with_file_name` が `\` を作り、`cli::a_miscased_entry_path_is_still_the_entry` が E1030 でなく E1027 になる、と読める
  - std はコンソールの `ReadConsoleW` が Ctrl-C で戻ってもその場で読み直す (rust-src 1.98.1 `library/std/src/sys/stdio/windows.rs:353-394` の `read_u16s`)
  - rustyline 18.0.1 は Windows の raw mode で `ENABLE_PROCESSED_INPUT` を外し、Ctrl-C を `Cmd::Interrupt` にする (`src/tty/windows.rs:763-766`、`keymap.rs:1034`)
- libm を使う extern は `sin`、`cos`、`exp`、`log`、`pow` である (`crates/eml_extern/src/lib.rs:884-888`)。期待値に出るのは `tests/ui/run/float/std_float.em` (正確な値と、正しく丸めた `exp 1.0`) と `nbody_small.em` (9桁で表示。`pow` を `sqrt` の形に替えても macOS で同じ出力) だけである。bench の programs は `sqrt` と四則だけを使う
- `windows-sys` 0.61.2 はすでに Cargo.lock にある (ほかの crate の依存)。feature は `Win32_System_Console`、`Win32_System_Threading`、`Win32_System_SystemInformation` を使い、`Win32_Foundation` は `Win32` に含まれる。`BOOL` は `windows_sys::core::BOOL` (i32)
- runner の rustup は ubuntu-24.04 と windows-2025 が 1.29.1、macos-15 が 1.29.0 である。image の stable は 1.99.0、1.99.1、1.98.1 なので、版の比べで固定した toolchain と image のものを見分けられる (actions/runner-images の readme)
- 引用の検査 (`crates/eml_cli/tests/citations.rs`) は `docs/plans` を除いて読む。この計画と付録は引用の検査に掛からない
- 基点の開発機での計時: `cargo nextest run` は 3507 passed、18 skipped。いちばん遅いテストは `ui::run` の 0.76 s。Verus は 1043 verified、0 errors、7 warnings。Miri は `trusted::` 108 passed、`miri::` 10 passed (2つの設定とも)

## 4. 前提

| 要るもの | 使う所 | 確かめ方 | ないとき |
|---|---|---|---|
| devShell (nix、direnv) | すべて | `nix develop -c cargo --version` → `cargo 1.98.1` | 止めて利用者に聞く |
| Verus と verusfmt | T-6、T-10、T-11 | `cargo verus verify -p eml_rt` → 0 errors | 止めて聞く。`eml_rt` を変える T-6 は通せない |
| Miri の devShell | T-11 | `nix develop .#miri -c cargo miri --version` → 版が出る | 止めて聞く |
| 別の target の rust-std (nix で取る) | T-2 以降の Windows と Linux の clippy | 付録の `xclippy.sh <worktree>` → `Finished` が2回 | 初回は nix がダウンロードする。失敗したら止めて聞く |
| actionlint と shellcheck | T-11 | `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- --version` | 止めて聞く |
| `gh` の認証 | T-13 | `gh auth status` → logged in | 利用者に `! gh auth login` を頼む |
| push と PR の許可 | T-13 | 利用者に確かめる | T-12 までで止め、T-13 を利用者に渡す |
| Windows の実機 | なし (手での確かめは任意。9) | なし | 要らない |

## 5. 決めたこと

### 5.1 長く守る方針 (変えるには見直しが要る)

実装がこれと食い違ったら、止めて報告する。

#### D-1: Windows で流さないテストの基準

- 決め: テストは移植できるのが既定である。Windows での違いが処理系の側にあるなら、ソースを直すかテストを移す。印を付けて Windows で流さないのは、確かめる動きが Windows に無いときだけである。POSIX のツールを子のプロセスにすること、unix の signal、shebang での実行、fifo、unix の権限のビットがそれに当たる。Linux には印を付けない。機械に入っている道具の有無で飛ばすことはしない。次の3つには印を付けず、どの OS でも通るように直す
  - libm の最後のビットに依る期待値: 正確な場合か、IEEE で正確な演算 (`sqrt`、四則) に書き直す。許容の幅は入れない
  - スタックのあふれ: `eml.exe` なら、main.rs でコマンドの本体を 8 MiB のスタックのスレッドで動かす (macOS と Linux のメインスレッドと同じ大きさ)。そうでなければ再帰を直す
  - OS が作る誤りの文言: fixture を替え、文言が Rust か eml から来るようにする
- 理由: テストの合否が機械によってはならない (CLAUDE.md、testing.md 「性能のテスト」)。この一つで決まる。runner の PATH に Git for Windows の道具があるかどうかは判断に使わず、docs にも書かない
- 採らなかった案: PATH に道具があるときだけ Proc の UI テストを流す (合否が機械による)、`cmd /c` の版の Proc テスト (OS ごとの snapshot になる)
- 及ぶ範囲: すべてのテスト
- 見直す条件: `io-driver` の段が、`eml` 自身を子にする移植できる Proc テストを入れるとき (Q-2)
- リポジトリに残す所: testing.md の新しい節 「OS ごとのテスト」 (T-5)

#### D-2: 印の書き方と読む所

- 決め:
  - Rust のテストは、`#[test]` の関数か、そうしたテストだけを持つ `mod` に `#[cfg(unix)]` を付ける。今の `terminal.rs::interrupts` と `eml_vm` の `hosts.rs::tests` の印はそのまま残す
  - UI テストと REPL のトランスクリプトは、入口のファイルの、shebang (`#!…`) でない最初の行に印を書く。`.em` (1つのファイルか `main.em`) は `-- windows: skip (<理由>)`、`.repl` (1つのファイルか `main.repl`) は `# windows: skip (<理由>)` である。理由は空にしない。印は入口のファイルにだけ書く
  - `crates/eml_cli/tests/common/mod.rs` の `pub fn windows_skip(line, leader) -> Result<bool, String>` が印を読み、`for_each_file` が `EML_UI_FILTER` の後で使う。leader は拡張子 `repl` で `#`、ほかで `--` である。Windows (`cfg!(windows)`) では、印のあるファイルを流さない。どの OS でも、`<leader> windows:` で始まって形の合わない行は panic する。文言は ``the Windows mark is `-- windows: skip (<reason>)`, not `…` `` である
  - 一覧は印そのものである。docs には規則と次の2つの数え方を書き、一覧は写さない
    - `git grep -n '^-- windows: skip (' -- tests/ui; git grep -n '^# windows: skip (' -- crates/eml_cli/tests/repl`
    - `git grep -n -A1 'cfg(unix)\]' -- 'crates/*/tests/*.rs' 'crates/*/src/**/tests/*.rs' crates/eml_vm/src/hosts.rs`
- 理由: Rust のテストの本体は `libc::kill`、`mkfifo`、`PermissionsExt`、`CommandExt::process_group` を使うので Windows でコンパイルできず、`cfg_attr(windows, ignore)` は使えない。UI テストの印は理由がテストの中にあり、grep で数えられ、書き損じがどの OS でも落ちる。14本に印を付けても snapshot は変わらない (backtrace は `<std>/Proc.em` だけを指す。試作で確かめた)
- 採らなかった案: runner か `.config/nextest.toml` の一覧 (テストから離れてずれる)、`tests/ui/run/unix/` のような OS のディレクトリ (14の snapshot の名前が変わり、分類が機能でなく OS になる)、`<name>.windows-skip` の横置きのファイル (今の横置きの `.args`、`.stdin`、`.env` はプログラムへの入力で、テストの性質ではない。トランスクリプトには横置きがない)
- 及ぶ範囲: UI テスト、トランスクリプト、Rust のテスト
- 見直す条件: ネイティブの段が `// native: skip` の印を入れるとき。その段で、eml のコメントは `--` であることと、この印の形に合わせることを決める (Q-4)
- リポジトリに残す所: testing.md 「OS ごとのテスト」、root の CLAUDE.md の Testing の UI テストの箇条 (T-5)

#### D-3: 改行は変換しない

- 決め: root の `.gitattributes` を次の2行にする。引用は書かない

  ```
  # 改行を変換しない。CRLF を含むテストの入力をそのまま保つ
  * -text
  ```
- 理由: `text=auto` は CRLF の fixture 8つも正規化する。`-text` なら、`core.autocrlf` によらず checkout の中身が同じになる
- 及ぶ範囲: リポジトリのすべてのファイル
- リポジトリに残す所: `.gitattributes` (T-1)、testing.md 「OS ごとのテスト」 (T-5)

#### D-4: toolchain の版は `rust-toolchain.toml` に1か所だけ書く

- 決め: `rust-toolchain.toml` を次のとおりにする。flake は `channel` をこのファイルから読み、`sha256` は flake.nix に残す

  ```toml
  # Verus が求める版に固定する。flake も channel をここから読む (docs/implementation/testing.md の「Verus」)
  [toolchain]
  channel = "1.98.1"
  components = ["clippy", "rustfmt"]
  profile = "minimal"
  ```

  ```nix
  stable = fenix.toolchainOf {
    channel = (pkgs.lib.importTOML ./rust-toolchain.toml).toolchain.channel;
    sha256 = "sha256-p8h3Sl/YRByZfZTAKXdsvF6xEenXKrXSVvpphmZENH4=";
  };
  ```
- 理由: 版が1か所なので、同じことを確かめるテストが要らない。`channel` だけを変えて `sha256` を変え忘れると、評価が hash の食い違いで止まる (試作で確かめた)。devShell の `cargo` は fenix のもので rustup を通らないので、ファイルは shell の中で効かない。drvPath は変わらない (試作で確かめた)
- 採らなかった案: fenix の `fromToolchainFile` (components もファイルから取るが、devShell は rust-src と rust-analyzer も要る)、flake と CI に版を別々に書いて照らすテスト
- 及ぶ範囲: flake、CI、nix の外の rustup の利用者
- 見直す条件: Verus の版を上げるとき (2つの URL、`channel`、`sha256` を一緒に変える)
- リポジトリに残す所: `rust-toolchain.toml`、flake.nix のコメント、architecture.md の toolchain の箇条、testing.md 「Verus」、CLAUDE.md の Commands (T-10)

#### D-5: CI は合否だけを見る

- 決め:
  - CI が見るのは合否だけである。1つのテストの時間の条件 (1 s) と Miri の CPU 時間の予算は開発機で確かめ、runner の速さに合わせて緩めない
  - nextest の CI 用の profile は作らない。落ちたテストは流し直さない。合否が速さや順序で変わるテストは、流し直しで隠さずに直す
  - `#[ignore]` のテストと `bench/` は CI で流さない
  - アクションは commit の SHA で固定し、版の名前をコメントに書く。runner の image は名前で固定する (`ubuntu-24.04`、`macos-15`、`windows-2025`)。上げるのは手で行う
  - Verus、verusfmt、Miri は Linux で nix を通して流す。ふだんのテストと clippy は、3つの OS で runner の rustup を通して流す
- 理由: ローカルの計時で、いちばん遅いテストは 0.76 s で、nextest がテストを止める 60 s の約 1/80 である。CI 用の profile は5つの重いテストの指定を二重に持つだけになる。Linux のテストを nix で流すと、flake を2回試し、toolchain のファイルを Linux で一度も試さない
- 採らなかった案: `[profile.ci]` (長い時間切れ、流し直し、JUnit)、`-latest` の image、Dependabot
- 及ぶ範囲: CI と testing.md
- 見直す条件: `native-aot` が release の成果物を作るとき (`nix build` を CI に入れるか)、`net-windows` が IOCP のテストを足すとき
- リポジトリに残す所: testing.md 「方針」の箇条と新しい節 「CI」、CLAUDE.md の Testing と Commands (T-11)

#### D-6: Ctrl-C の受け口の形

- 決め:
  - `crates/eml_cli/src/sigint.rs` を bin のモジュール `interrupt` に替える。`interrupt/mod.rs` (共通)、`interrupt/unix.rs`、`interrupt/windows.rs` の3つのファイルにする
  - `mod.rs` は `Mode {Run, Repl}`、`install`、`static INSTALLED: OnceLock<Installed>` (`Installed { control: Arc<Control>, mode: Mode }`。今の `CONTROL` と `EXIT_ON_REPEAT` を替える)、`Installed::respond(&self) -> Response { Interrupted | Exit }`、`const REPEAT_EXIT_CODE: u8 = 130` を持つ。`respond` は、`Run` で `INTERRUPT` がすでに立っていれば `Exit` を返し、ほかでは `control.interrupt()` を呼んで `Interrupted` を返す。両方の OS が使う判断はこれだけである
  - `install` は `INSTALLED` を置いてから `register()` を呼ぶ。2回目の `install` は assert で落とす ("the Ctrl-C handler is installed once per process")。`register` が失敗したら `warning: cannot install the Ctrl-C handler: {error}` を書く
  - OS の入口の関数は1行の殻にし、判断はすべて `respond` か、Windows の `console_event` に置く。テストは `install` を呼ばず、`INSTALLED` に触れない。`interrupt::tests::installed(mode)` で `Installed` を作って渡す
  - Win32 の binding は `windows-sys` だけを使い、`eml_cli` にだけ入れる。`libc` は `eml_cli` で `[target.'cfg(unix)'.dependencies]` に移す。`eml_rt` の依存は変えない (vstd だけ)
  - eml_cli の `[[bin]]` から `test = false` を外し、bin の単体テストのバイナリを作る
- 理由: handler の本体が入力を引数で受けるので、テストが static もプロセスの終わりも使わずに本物の判断を通せる。`TERMINATE` が入るときは、`respond` が signal の引数を持ち、`console_event` が CLOSE と SHUTDOWN をそれに写し、unix は `unix.rs` に自己パイプを足すだけで済む
- 採らなかった案: `ctrlc` crate (Windows の handler が事象の種類を見ずに TRUE を返し、待ちのスレッドを足し、`SA_RESTART` を外した `sigaction` を置き換える)、`cfg(test)` で終わる関数を差し替える (引数で渡せば試験だけの道が要らない)、ほかの OS 向けの空の `register` (対応しない OS は、警告もなく Ctrl-C を無視するより、コンパイルで落ちるほうがよい)
- 及ぶ範囲: eml_cli の bin、Cargo の依存
- 見直す条件: `TERMINATE`、監視のスレッド、自己パイプを入れる段
- リポジトリに残す所: vm.md 「上限と割り込み」、architecture.md、testing.md、crates/eml_rt/CLAUDE.md (T-3、T-4)

#### D-7: Windows の target は msvc だけにする

- 決め: Windows で対応する target は `x86_64-pc-windows-msvc` だけである。`x86_64-pc-windows-gnu` (MinGW) は対応せず、CI でも別の target の clippy でも流さない。CI の Windows のジョブは、`rustc -vV` の `host:` が `x86_64-pc-windows-msvc` であることを確かめる
- 理由: 利用者の指示である。後のネイティブ化の設計も Windows で Visual Studio の `link.exe` (あれば `lld-link`) を使う ([ネイティブ化の設計](../future/native.md) の「AOT と配布」)
- 及ぶ範囲: CI、付録の `xclippy.sh`、docs
- 見直す条件: 利用者が gnu の対応を求めたとき
- リポジトリに残す所: testing.md 「OS ごとのテスト」 (T-5) と 「CI」 (T-11)

### 5.2 短中期の設計

理由があれば実装の中で変えてよい。変えたときはコミットのメッセージに書く。

#### D-10: Windows の handler の動き

- `CTRL_C_EVENT` には TRUE を返す。1回目は `INTERRUPT` を立てる。`Run` で `INTERRUPT` が立っていれば `TerminateProcess(GetCurrentProcess(), 130)` で終わり、まだ書いていない出力は `_exit` と同じく捨てる。`Repl` の2回目はビットを立て直すだけである
- `CTRL_BREAK_EVENT`、`CTRL_CLOSE_EVENT`、`CTRL_LOGOFF_EVENT`、`CTRL_SHUTDOWN_EVENT` とほかの値、`INSTALLED` がまだないときの Ctrl-C には FALSE を返す。OS の既定の handler がプロセスを終わらせ、まだ書いていない出力は捨てる。Ctrl-Break は、REPL が抜けられない処理から抜ける道で、unix の Ctrl-\ に当たる
- 終了コードはどちらの OS も 130 である。1回目は main.rs の今の `error.interrupted()` → 130 の道を通る
- handler のスレッドは OS が作り、プログラムと並んで動く。`OnceLock` を1つ読み、Relaxed のアトミックを使い、`TerminateProcess` を呼ぶだけで、ロックも確保もコンソールの I/O もしない
- `respond` は確かめてから立てるので、不可分でない。続けて押した2回の handler が同時に動くと、どちらも1回目として扱い、次の Ctrl-C で `eml run` が終わる。これは受け入れ、vm.md に書く。unix の handler は入れ子にならないので、この重なりはない
- `ExitProcess` でなく `TerminateProcess` を使うのは、ほかのスレッドがロックを持つ間の DLL の切り離しを避けるためである (推測)。自分のコードがプロセスを終わらせる所だけで `TerminateProcess` を選び、ほかの事象は、どのコンソールのプログラムも受ける OS の既定に任せる

#### D-11: Windows の待っている読みは Ctrl-C で止まらない

- コンソールの標準入力の読みは Ctrl-C で戻るが、std がその場で読み直す。pipe とファイルの読みと書きは Ctrl-C で戻らない (推測)。標準入力、`FileIn`、`Child` の読みは、データが来るか相手が閉じて読みが戻った後で止める語を見る。`eml run` は、待っている読みからも2回目の Ctrl-C で抜けられる
- 子のプロセスを待つ間の Ctrl-C は同じコンソールにつながる子にも届き、子が終わって `Proc.wait` が止まる
- REPL の行の編集の間の Ctrl-C は、rustyline が `ReadlineError::Interrupted` にし、handler は呼ばれない (推測)。terminal.rs はこの道をすでに扱う
- std を通さない読み手は入れない (Q-1)。multicore.md:564 の未決の項目を、今の動きを書いた形に書き直して残す

#### D-12: ディレクトリはどの OS でも開かない

- `StdHost::open` は、開いたファイルの `metadata().is_dir()` を見て `ErrorKind::IsADirectory` を返す。大きさの見積りのためにすでに取っている metadata を使う
- `open` と `open_write` は、開けなかった誤りを `directory_error(path, e)` に通し、path がディレクトリならその誤りを `IsADirectory` に分け直す。成功した `open` に費用を足さず、確かめてから開くまでの競合もない (unix は開いた handle の `fstat`、Windows は開けなかった後だけ `metadata`)
- 誤りの表示は、どの OS でも ``cannot open `x`: I/O error`` である
- コードは `verus!` の外 (host.rs:24-569 の外) で、std だけを使う。それでも `eml_rt` の変更なので、Verus と verusfmt の確認を流す
- 採らなかった案: `read_directory.em` に印を付け、`dir` の表の2行を Windows で飛ばす (利用者に見える動きが OS で変わり、spec に書かれない)、Windows だけ `io_reason` で分け直す (誤りを `read` から `open` へ移せず、表示に操作の名前が入る)

#### D-13: トランスクリプトの fixture

- `crates/eml_cli/tests/repl/commands/load/main.repl` の `> :load missing.em` を `> :load not_utf8.txt` にし、バイト `61 ff 0a` の `not_utf8.txt` を足す。期待値の文言が Rust の `stream did not contain valid UTF-8` になり、OS によらない。「失敗した `:load` の後も前に読んだファイルの定義が残る」ことを確かめる範囲は変わらない。ファイルがない場合は `terminal.rs::an_unreadable_load_keeps_the_session` が OS によらずに確かめる
- 拡張子は `.em` にしない。引用の検査は `.rs .em .md .py .sh` を UTF-8 として読むためである

#### D-14: 入口の名前は `/` で区切る

- main.rs に `fn entry_name(path: &Path) -> String` を置く。`exact_spelling(path)` を表示の文字列にし、`cfg!(windows)` のとき `\` を `/` に替える。`eml run` と `eml check` の `load` (main.rs)、REPL の `eml repl FILE` と `:load` (terminal.rs:38、:163) で使う。import の根は `Path::new(&shown).parent()` から取る
- Windows で `a_miscased_entry_path_is_still_the_entry` と、`modules_are_read_from_the_entry_directory` の絶対パスの確かめを通し、`eml run C:\x\main.em` の欠陥も直す
- `a_miscased_entry_path_is_still_the_entry` は、大文字と小文字を区別しないファイルシステム (macOS、Windows) でだけ規則を確かめ、Linux では早く戻る。OS ごとに決まっていて合否は機械によらないので、印は付けず testing.md に書く

#### D-15: Windows のフォールバック

- `physical_memory` は Windows で `GlobalMemoryStatusEx` の `ullTotalPhys` を読む (feature `Win32_System_SystemInformation`)。`api.rs` の2つのテストの `if cfg!(any(target_os = "macos", target_os = "linux"))` を外し、Windows の CI がこの道を通るようにする
- `inherited_stdout` は `Unsupported` のまま残す。Windows でそれを通すテストがないためである (Q-2)。stdlib.md に、Windows の `run (merge_stderr c)` は ``cannot run `x`: not available`` になると書く
- `exit_code` の 128 + signal は unix だけで、stdlib.md に書く。`stdout_width` の 80 はすでに書いてある
- `table.rs::locked()` は Windows で false のままで、`locked.txt` の行は root のときと同じく Windows でも飛ばす。testing.md 「期待値の表」に書く

#### D-17: ワークフローの形

- ファイルは `.github/workflows/ci.yml` の1つで、ジョブは4つである。全文は付録の `s2-ci.patch` にあり、actionlint と shellcheck を通してある
  - `test`: `ubuntu-24.04`、`macos-15`、`windows-2025` の matrix (`fail-fast: false`)。runner の rustup で `rustup toolchain install` (引数なし) を流し、`rustc -V` の版が `rust-toolchain.toml` の `channel` と同じかを確かめる。Windows では、`rustc -vV` の `host:` が `x86_64-pc-windows-msvc` であることも確かめる (D-7)。rust-cache の後、taiki-e/install-action で `cargo-nextest@0.9.146` を `fallback: none` で入れる。`cargo nextest run --failure-output immediate-final`、`cargo clippy --all-targets -- -D warnings`、既定でない feature の clippy 7つ (`eml_cli` の none、types、core と、`eml_test_support` の none、hir、types、core。lib だけで `--all-targets` なし) を流す。clippy の手順は `!cancelled() && steps.test.outcome != 'skipped'` で、テストが落ちても流す
  - `fmt`: ubuntu で `cargo fmt --check`
  - `verus`: ubuntu で cachix/install-nix-action を入れ、`nix develop -c cargo verus verify -p eml_rt` と、`git ls-files 'crates/eml_rt/src/*.rs'` を渡した `nix develop -c verusfmt --verus-only --check`
  - `miri`: ubuntu で、借用の規則ごとに1つのジョブ (stacked は `-Zmiri-strict-provenance`、tree は `-Zmiri-strict-provenance -Zmiri-tree-borrows`)。`nix develop .#miri -c cargo miri test -p eml_rt trusted::` と `nix develop .#miri -c cargo miri test -p eml_vm --test integration miri::` を続けて流す
- 起動は main への push、pull_request、workflow_dispatch である
- concurrency の組は `${{ github.workflow }}-${{ github.ref == 'refs/heads/main' && github.sha || github.ref }}`、`cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}` である。1つの組では待つ実行が1つだけで、新しい実行が古い待ちを取り消すので、main はコミットごとに組を分け、どのコミットにも結果を残す
- `permissions: contents: read`。checkout は `persist-credentials: false`。shell はどの OS も bash にする
- 時間切れ (分): test 60、fmt 10、verus 30、miri 45
- アクションの固定: actions/checkout v7.0.1 `3d3c42e5aac5ba805825da76410c181273ba90b1`、Swatinem/rust-cache v2.9.2 `6323deb102c322ba6fcbdcafc7e3dddab59af2b6`、taiki-e/install-action v2.87.26 `f7e5d7c961414b23f5b25b2da9294395d08513ad`、cachix/install-nix-action v31.11.1 `13d8dd58da0234aa297dedd986986ccb8e7f3e24`
- rust-cache は `test` だけで使い、保存は main の実行だけ (`save-if`) にする。nix のストアはキャッシュしない
- install-nix-action には `github_access_token: ${{ secrets.GITHUB_TOKEN }}` を渡す。`github:` の input の取得が率の制限に当たらないためである。token は nix.conf に入り、ジョブの中の `nix develop` で動くものが読めるが、`contents: read` で public のリポジトリで、ジョブとともに切れるので受け入れる。install-action には token を渡さない (`fallback: none` では使わない)
- 採らなかった案: actions-rust-lang/setup-rust-toolchain (環境変数の既定を足し、ローカルと CI が違う)、dtolnay/rust-toolchain (toolchain のファイルを読まない)、DeterminateSystems の installer (upstream でない Nix を入れる)、magic-nix-cache (今の状態を確かめられない)、すべての branch の push での起動 (PR と同じコミットを2回流す)

#### D-18: 引用の検査が設定のファイルも読む

- `citations.rs` の root の `files` に `flake.nix`、`rust-toolchain.toml`、`.config/nextest.toml`、`.github/workflows/ci.yml` を足し、`dirs` に `root.join("nix")`、拡張子に `"nix"` を足す。`#` はすでにコメントの印として外す
- 新しいファイルが docs の見出しを引くが、今はそれを確かめるものがないためである
- 既存のテストの読む範囲を広げる変更で、テストの変更の3つの種類のどれにも当たらない。動かすテストも期待値もないが、足したファイルの古い引用で新しく落ちうるためである。この計画で名指しし、計画の承認をその合意にする。足すファイルは今すべて通る

## 6. 実装の決まり

- 期待値と合否を変えるのは、8 の「テストの変更の一覧」にあるものだけである。期待値をすべて同じに保つ機械的な追随は認める (D-1)
- 日本語のコメントと docs は yomiyasu の規則に従う。コードと docs は `docs/plans/` を引かない。spec の節は、規則を実装するコミットと同じコミットで書く (CLAUDE.md)
- docs には段の名前、日付、前の状態を書かない
- Windows と Linux の clippy は、付録の `xclippy.sh` で流す。link はしないので、通ってもビルドの証明にならない (D-5)
- `eml_rt` を変えたら、`cargo verus verify -p eml_rt` (0 errors、7 warnings) と `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` を流す (crates/eml_rt/CLAUDE.md)
- snapshot は `INSTA_UPDATE=always` と `EML_UI_FILTER` で、対話なしに作り直す。`cargo insta review` は使わない
- テストは `install` を呼ばず、`INSTALLED` に触れない。`on_console_event` と `on_sigint` は1行の殻のままにする。`on_sigint` を直接呼ぶテストは足さない。それは cli.rs と terminal.rs の端から端までのテストが確かめる (D-6、testing.md 「重複させない」)
- `windows-sys` の行はワークスペースの Cargo.toml に1つだけ置く。試作の `s1-tests.patch` は単独で書いたので2つ目の行を持つ。T-7 では使わない (付録の使い方)
- エージェントは push も PR も作らない。T-13 は利用者の確かめの後に行う

## 7. 実装の作業

| ID | 作業 | 基づく決め | 先に要る作業 | 並べられるか |
|---|---|---|---|---|
| T-1 | `.gitattributes` | D-3 | - | はい |
| T-2 | Windows の lint と、UTF-8 でない値のテストの移植 | D-1 | - | はい |
| T-3 | Ctrl-C の受け口を共通の部分と unix に分ける | D-6 | - | はい |
| T-4 | Windows のコンソールの handler | D-6、D-10、D-11 | T-3 | いいえ |
| T-5 | Windows の印 | D-1、D-2、D-7 | T-1 | いいえ |
| T-6 | ディレクトリを開かない | D-12 | T-5 | いいえ |
| T-7 | Windows の物理メモリ | D-15 | T-4 | いいえ |
| T-8 | トランスクリプトの fixture | D-13 | T-5 | いいえ |
| T-9 | 入口の名前の `/` | D-14 | T-2 | いいえ |
| T-10 | toolchain の固定 | D-4 | - | はい |
| T-11 | CI のワークフローと docs | D-5、D-7、D-17 | T-1〜T-10 | いいえ |
| T-12 | 引用の検査が設定のファイルを読む | D-18 | T-11 | いいえ |
| T-13 | CI を流し、初回の失敗を直す | D-5、D-17 | T-12 | いいえ |
| T-14 | 段を閉じる | すべて | T-13 | いいえ |

各作業は1つのコミットである。T-13 だけは、直しがあればその数だけコミットになる。

### 付録の使い方

- `s1-tests.patch`、`s3-interrupt.patch`、`s2-ci.patch` は、基点にこの順で当てた試作である (`git apply --binary` で当て、1つずつコミットしてから次を当てる)。3つを当てた木で、`cargo nextest run` は 3511 passed、1 failed (まだ書いていない 「OS ごとのテスト」 を引く `citations::every_cited_heading_exists`)、clippy は macOS と `xclippy.sh` の target (当時は Windows の gnu を含む3つ) で通った
- patch は作業ごとに分けていない。作業の範囲に当たる hunk だけを取り、この計画の文に合わせて直す。patch と計画が食い違ったら計画に従う
- patch の日本語は yomiyasu を通していない。docs の文は、S3 の部分 (`s3-interrupt.patch`) は確定した文として使ってよい。ほかは下書きとして扱い、書き直す
- `xclippy.sh <worktree の絶対パス> [clippy の引数...]` は、`xtarget.nix` で flake と同じ fenix の input と版から Windows と Linux の rust-std を取り、2つの target で clippy を流す。引数を省くと `--workspace --all-targets -- -D warnings` になる。target の出力は `<worktree>/target/xclippy` に置く。期待は `== <target>` の後の `Finished` が2回である
- `xtarget.nix` は版と sha256 を自分で持つ。T-10 の後もこのファイルは変えない (付録は段とともに消える)

### T-1: `.gitattributes`

- 目的: Windows の checkout で CR を入れない
- 対象: `.gitattributes` (新しいファイル)
- 作業: D-3 の2行を書く
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `git check-attr text -- tests/ui/run/io/stdin_handles.stdin` → `tests/ui/run/io/stdin_handles.stdin: text: unset`
  - `git clone -q -c core.autocrlf=true . <scratch>/crlf && git -C <scratch>/crlf ls-files -z | (cd <scratch>/crlf && xargs -0 grep -lI $'\r') | wc -l` → `8` (`<scratch>` は作業の外の一時ディレクトリ。コミットの後に流す)
- 落とし穴: 説明の行に docs の節を引かない。節は T-5 で書くので、引用の検査が落ちる

### T-2: Windows の lint と、UTF-8 でない値のテストの移植

- 目的: Windows の clippy をきれいにし、2つのテストを Windows でも流す
- 対象: `crates/eml_rt/src/heap/tests/script.rs`、`crates/eml_cli/tests/cli.rs`、docs/implementation/testing.md 「CLI のテスト」
- 作業:
  - `script.rs`: `cfg(windows)` の `not_utf8()` を足す。`OsString::from_wide(&[0x61, 0xD800])` (孤立した surrogate) を作る。3つの部分的な `cfg(unix)` を外す。これで `script.rs:77` の `unused_mut` も消える
  - `cli.rs`: unix と windows の `not_utf8()` を置き、`arguments_and_variables_that_are_not_utf8_are_runtime_errors` の `cfg(unix)` を外す
  - `cli.rs::run_opens_an_absolute_path_as_is`: eml の文字列リテラルに埋めるパスの `\` を2つにする
  - testing.md 「CLI のテスト」: UTF-8 でない値のテストがもう `cfg(unix)` でないことを書く
  - 試作: `s1-tests.patch` の `script.rs` と `cli.rs` の hunk
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_rt script` → すべて passed
  - `cargo nextest run -p eml_cli --test integration -E 'test(cli::)'` → すべて passed
  - `cargo clippy --all-targets -- -D warnings` → `Finished`、警告なし
  - `xclippy.sh <worktree>` → `Finished` が2回
- テストの変更: 種類3 (期待値は変わらない)

### T-3: Ctrl-C の受け口を共通の部分と unix に分ける

- 目的: unix の動きを変えずに、D-6 の形を作る
- 対象: `crates/eml_cli/src/sigint.rs` (消す)、`crates/eml_cli/src/interrupt/mod.rs`、`interrupt/unix.rs`、`crates/eml_cli/Cargo.toml`、`main.rs`、`terminal.rs`、`lib.rs:280` のコメント、vm.md、architecture.md、testing.md
- 作業:
  - `interrupt/mod.rs` を `cfg(windows)` の行なしで、`interrupt/unix.rs` を D-6 のとおりに作る。`on_sigint` は `if let Some(Response::Exit) = INSTALLED.get().map(|i| i.respond()) { libc::_exit(130) }` で、`sigaction` の flags は今と同じにする
  - この作業では、`mod interrupt;` と2つの呼ぶ所の `#[cfg(unix)]` を残す
  - `[[bin]]` から `test = false` を外し、次の4つのテストを `mod.rs` に足す。どの OS でも流れる
    - `interrupt::tests::the_first_ctrl_c_sets_the_interrupt_bit` (Run と Repl)
    - `interrupt::tests::a_second_ctrl_c_ends_a_run`
    - `interrupt::tests::a_second_ctrl_c_only_interrupts_the_repl`
    - `interrupt::tests::a_ctrl_c_after_a_cleared_interrupt_is_a_first_one`
  - docs: vm.md:341、architecture.md:361、lib.rs:280 の `sigint.rs` と `sigint` モジュールを `interrupt` モジュールにする。testing.md:58 から 「と `eml_cli` の bin」 を消す。testing.md に bin の単体テストの段落 (`s3-interrupt.patch` の testing.md の追加) を、Windows の文を除いて足す
  - 試作: `s3-interrupt.patch` の `interrupt/mod.rs`、`interrupt/unix.rs` と、上の docs の hunk
- 実装者に任せること: なし
- 条件: いつも。T-4 と1つのコミットにまとめてもよい
- 完了の条件:
  - `cargo nextest run -p eml_cli` → 基点の数 + 4 がすべて passed (`eml_cli::bin/eml interrupt::tests::*` の4つを含む)。unix の SIGINT の端から端までのテストも通る
  - `cargo clippy --workspace --all-targets -- -D warnings` → `Finished`、警告なし
  - `cargo fmt --check` → 出力なし
  - `rg -n 'sigint\.rs|mod sigint|sigint::' crates/eml_cli/src docs/spec docs/implementation` → 何も出ない (`on_sigint` は SIGINT の handler なので名前を残す)
- テストの変更: 新しいテストだけ。bin の `test = false` を外すのは、testing.md 「crate の中の置き方」の決まりの変更で、この計画の承認を合意にする。`cargo nextest run` ごとにテストのバイナリが1つ増える

### T-4: Windows のコンソールの handler

- 目的: Windows で Ctrl-C を受ける (D-10、D-11)
- 対象: `crates/eml_cli/src/interrupt/windows.rs`、`interrupt/mod.rs` の `cfg(windows)` の行、ワークスペースと eml_cli の Cargo.toml、Cargo.lock、`main.rs`、`terminal.rs`、docs
- 作業:
  - `windows.rs` に、`register()` (`SetConsoleCtrlHandler(Some(on_console_event), TRUE)`)、`unsafe extern "system" fn on_console_event(event: u32) -> BOOL { console_event(INSTALLED.get(), event, terminate) }`、`fn console_event(installed: Option<&Installed>, event: u32, exit: impl FnOnce(u32)) -> BOOL`、`fn terminate(code: u32)` (`TerminateProcess(GetCurrentProcess(), code)`) を置く。`console_event` の判断は D-10 のとおりである
  - 依存: ワークスペースに `windows-sys = { version = "0.61.2", features = ["Win32_System_Console", "Win32_System_Threading"] }`。eml_cli に `[target.'cfg(unix)'.dependencies] libc = { workspace = true, optional = true }` (`[dependencies]` から移す) と `[target.'cfg(windows)'.dependencies] windows-sys = { workspace = true, optional = true }`。`run = ["core", "dep:eml_vm", "dep:libc", "dep:rustyline", "dep:windows-sys"]`
  - `mod interrupt;` と2つの呼ぶ所の `#[cfg(unix)]` を外す
  - `cfg(windows)` のテストを2つ足す。`console_event` を直接呼び、`exit` には記録する closure を渡す
    - `interrupt::windows::tests::the_console_handler_takes_ctrl_c_and_ends_a_run_on_the_second`: `Run` の1回目の `CTRL_C_EVENT` は TRUE を返し、ビットが `INTERRUPT` で、`exit` は呼ばれない。2回目は TRUE を返し、`exit` が 130 で呼ばれる
    - `interrupt::windows::tests::the_console_handler_leaves_the_other_events_to_the_system`: BREAK、CLOSE、LOGOFF、SHUTDOWN はどれも FALSE を返し、ビットは 0 のままで、`exit` は呼ばれない (呼ばれたら closure が panic する)。`console_event(None, CTRL_C_EVENT, ..)` も FALSE を返す
  - docs: `s3-interrupt.patch` の残りの docs の文をそのまま入れる。対象は vm.md 「上限と割り込み」、repl.md 「割り込み」と 「残る制限」 (新しい6)、object-model.md (実行の制御、`FileIn.read_all`、子のプロセス、出力の buffer)、stdlib.md (`Proc`、`Child`)、architecture.md (依存の表の `windows-sys` の行を含む)、testing.md の Windows の文、multicore.md:564 の書き直し、crates/eml_rt/CLAUDE.md の1句である。root の CLAUDE.md と async.md は変えない
  - 試作: `s3-interrupt.patch` の残りのすべての hunk。ただしワークスペースの `windows-sys` の行は上の1行だけにする
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_cli` → すべて passed (Windows のテストは macOS では流れない)
  - `cargo clippy --workspace --all-targets -- -D warnings` → `Finished`、警告なし
  - `cargo clippy -p eml_cli --no-default-features -- -D warnings`、`--features types`、`--features core` → それぞれ `Finished`、警告なし
  - `xclippy.sh <worktree>` → `Finished` が2回
  - `xclippy.sh <worktree> -p eml_cli --all-targets -- -D warnings` → `Finished` が2回 (Windows のテストもコンパイルされる)
  - `cargo fmt --check` → 出力なし
- CI だけで確かめられること: 2つの Windows のテストが通ること、link できること、どのテストの stderr にも `warning: cannot install the Ctrl-C handler` が出ないこと
- 落とし穴: この警告が Windows の CI に出たら、`GetConsoleWindow()` (feature `Win32_System_Console`) が null でないときだけ `register` を呼ぶ。コンソールがなければ Ctrl-C の事象は来ないためである。それでも失敗したら、Windows では警告を書かずに誤りを捨て、vm.md の Windows の箇条に書く (T-13 の直し)
- テストの変更: 新しいテストだけ

### T-5: Windows の印

- 目的: Windows に無い動きを確かめるテストに印を付け、規則を docs に書く (D-1、D-2)
- 対象: `crates/eml_cli/tests/common/mod.rs`、`crates/eml_cli/tests/ui.rs`、下の14本の UI テスト、docs/implementation/testing.md、root の CLAUDE.md
- 作業:
  - `windows_skip` と、`for_each_file` での使用 (先に shebang の行を飛ばす) を足す
  - テスト `ui::the_windows_mark_is_the_first_line` を足す。2つの leader、印のない行、理由のない印、`()`、`windows:skip` を確かめる
  - 次の14本の1行目に `-- windows: skip (子のプロセスに POSIX のツールを使う)` を足す (理由は実装者が各テストに合う言い方にしてよい)。今の説明の行は2行目になる。パスは `tests/ui/` の下である
    1. `run-fail/std/proc_capture_nonzero.em`
    2. `run-fail/std/proc_capture_not_utf8.em`
    3. `run/io/try_io_cmd_exit.em`
    4. `run/std/child_stderr.em`
    5. `run/std/proc_args.em`
    6. `run/std/proc_capture.em`
    7. `run/std/proc_child_released_on_abort.em`
    8. `run/std/proc_cwd/main.em` (`./run.sh` の shebang も使う)
    9. `run/std/proc_env.em`
    10. `run/std/proc_spawn.em`
    11. `run/std/proc_spawn_large.em`
    12. `run/std/proc_stderr.em`
    13. `run/std/processes.em`
    14. `run/scripts/deploy/main.em`
  - 残す `cfg(unix)` の12のテスト (コードは変えない。8 の一覧に書く)
  - docs:
    - testing.md の 「CLI のテスト」の後に `## OS ごとのテスト` を足す。中身は次のとおり
      - 3つの OS で流すこと、Windows の target は `x86_64-pc-windows-msvc` だけであること (D-7)、テストは移植できるのが既定であること
      - Windows で流さない動きの種類 (D-1)
      - 印の形、読む所、書き損じを拒むこと (D-2)
      - 2つの数え方のコマンド
      - 機械の設定で飛ばさないこと
      - 印を付けない3つ (libm の最後のビット、スタックのあふれ、OS の誤りの文言) と、それぞれの直し方
      - `.gitattributes` の `* -text` とその理由
      - Linux には印がないこと
      - `locked.txt` の行 (root と Windows で飛ばす)
      - `a_miscased_entry_path_is_still_the_entry` は、大文字と小文字を区別しないファイルシステム (macOS、Windows) でだけ規則を確かめ、Linux では早く戻ること
    - testing.md 「2つのモードで流す」 (328 行目あたり): Windows では流さず印を付けることと、「OS ごとのテスト」への道しるべを足す
    - testing.md 「UI テスト」: 道しるべの箇条を1つ足す
    - root の CLAUDE.md の Testing の UI テストの箇条: "a UI test or transcript that cannot run on Windows carries the first-line mark (docs/implementation/testing.md の「OS ごとのテスト」)" を足す
  - 試作: `s1-tests.patch` の `common/mod.rs`、`ui.rs`、14本の `.em` の hunk。docs は試作にない
- 実装者に任せること: 印の理由の言い方。テストに合い、空でなければよい
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::) | test(citations::)'` → すべて passed
  - `git grep -l '^-- windows: skip (' -- tests/ui | wc -l` → `14`
  - `git diff --no-ext-diff --stat HEAD~1 -- crates/eml_cli/tests/snapshots` → 何も出ない (snapshot は変わらない)
  - `cargo nextest run` → すべて passed
- CI だけで確かめられること: Windows で14本が流れないこと
- テストの変更: 種類1。14本を Windows で流さない。12の `cfg(unix)` は記録だけで、合否は基点と同じ

### T-6: ディレクトリを開かない

- 目的: ディレクトリを開く動きを OS によらず同じにする (D-12)
- 対象: `crates/eml_rt/src/host.rs`、`crates/eml_rt/src/heap/tests/host.rs`、`crates/eml_rt/src/heap/tests/expected.txt`、`tests/ui/run-fail/files/read_directory.em` とその snapshot、docs
- 作業:
  - `StdHost::open` と `open_write` を D-12 のとおりに直す
  - `heap::tests::host::std_host::a_directory_opens_without_a_size` を消し、どの OS でも流れる `a_directory_does_not_open` を足す
  - `expected.txt` を作り直す: `cargo nextest run -p eml_rt --run-ignored ignored-only the_table_can_be_written`。変わるのは2行 (`f:"dir"` の `FileIn.read_all` と `read_line_raw`) だけである。`FileOut.create "dir"` はすでに `EISDIR` から `IsADirectory` なので変わらない
  - `read_directory.em` の1行目のコメントを直し、snapshot を作り直す: `EML_UI_FILTER=read_directory INSTA_UPDATE=always cargo nextest run -p eml_cli --test integration -E 'test(=ui::run_fail)'`。新しい表示は 6:11 の ``cannot open `.`: I/O error`` である
  - docs: object-model.md の `StdHost::open` の箇条 (1056 行目あたり) に 「ディレクトリは開かず、`I/O error` になる」。stdlib.md 「`FileIn`」と「`FileOut`」に、ディレクトリはどの OS でも ``cannot open `x`: I/O error`` になること。testing.md 「期待値の表」に 「読めないもの (Unix の 000。Windows と root では飛ばす)」
  - 試作: `s1-tests.patch` の `eml_rt` と `read_directory` の hunk
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo nextest run` → すべて passed
  - `git diff --no-ext-diff --stat HEAD~1 -- crates/eml_rt/src/heap/tests/expected.txt` → `1 file changed, 2 insertions(+), 2 deletions(-)`
  - `cargo verus verify -p eml_rt` → 0 errors、7 warnings
  - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 出力なし
  - `cargo fmt --check` → 出力なし
  - `xclippy.sh <worktree>` → `Finished` が2回
- テストの変更: 種類1 (`a_directory_opens_without_a_size` を消し、`a_directory_does_not_open` を足す)。種類2 (`expected.txt` の2行、`read_directory.em` の1行目のコメントと snapshot)。理由は D-12

### T-7: Windows の物理メモリ

- 目的: Windows でも確保量の上限を物理メモリから決める (D-15)
- 対象: `crates/eml_cli/Cargo.toml`、`crates/eml_cli/src/lib.rs`、`crates/eml_cli/tests/api.rs`、vm.md、stdlib.md
- 作業:
  - eml_cli の `windows-sys` の行に `features = ["Win32_System_SystemInformation"]` を足す (ワークスペースの行は T-4 のまま)
  - `cfg(all(feature = "run", not(unix)))` の `physical_memory` を、`GlobalMemoryStatusEx` と `ullTotalPhys` を使う `windows` の版に替える
  - `api.rs` の2つのテストから `if cfg!(any(target_os = "macos", target_os = "linux"))` を外し、doc コメントを書き直す
  - docs: vm.md の物理メモリの箇条 (T-4 の後の文) に、Windows は `GlobalMemoryStatusEx` で読むことを書き、`unsafe` を置く所の一覧にこの関数を足す。stdlib.md 「起動」に、終了コードの 128 + signal は unix だけであることと、Windows の `run (merge_stderr c)` は ``cannot run `x`: not available`` になることを書く
  - 試作: `s1-tests.patch` の `lib.rs` と `api.rs` の hunk
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_cli --test integration -E 'test(api::)'` → すべて passed
  - `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` と `--features core` → `Finished`、警告なし
  - `xclippy.sh <worktree>` → `Finished` が2回
- CI だけで確かめられること: Windows で2つの assert が通ること
- テストの変更: 種類2 (`api.rs` の2つの assert を Windows でも確かめる)

### T-8: トランスクリプトの fixture

- 目的: OS の誤りの文言を期待値から除く (D-13)
- 対象: `crates/eml_cli/tests/repl/commands/load/main.repl`、`not_utf8.txt` (新しいファイル)、`commands__load.snap`
- 作業: D-13 のとおりに替え、`printf 'a\xff\n' > crates/eml_cli/tests/repl/commands/load/not_utf8.txt` で fixture を作る。snapshot を作り直す: `EML_UI_FILTER=commands/load INSTA_UPDATE=always cargo nextest run -p eml_cli --test integration -E 'test(=repl::transcripts)'`
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `od -An -tx1 crates/eml_cli/tests/repl/commands/load/not_utf8.txt` → ` 61 ff 0a`
  - `cargo nextest run -p eml_cli --test integration -E 'test(repl::)'` → すべて passed
  - `git diff --no-ext-diff --stat HEAD~1 -- crates/eml_cli/tests/snapshots` → `commands__load.snap` だけが 2 insertions、2 deletions
- テストの変更: 種類2 (`commands__load.snap`)。消すテストの入力はない

### T-9: 入口の名前の `/`

- 目的: Windows の入口のパスの `\` を `/` として扱う (D-14)
- 対象: `crates/eml_cli/src/main.rs`、`terminal.rs`、docs/spec/modules.md (入口の読み込みの節。なければ repl.md)
- 作業: D-14 の `entry_name` を足して使う。docs に 「Windows では入口のパスの `\` を `/` として扱う」 を1文書く。試作は `s1-tests.patch` の `main.rs` と `terminal.rs` の hunk
- 実装者に任せること: docs の文を置く節。入口のパスの扱いを書いている節に置く
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_cli --test integration -E 'test(cli::) | test(terminal::)'` → すべて passed
  - `xclippy.sh <worktree>` → `Finished` が2回
- テストの変更: なし (期待値は変わらない。Windows で2つのテストが通るようになる)

### T-10: toolchain の固定

- 目的: 版を1か所に書く (D-4)
- 対象: `rust-toolchain.toml` (新しいファイル)、`flake.nix`、architecture.md、testing.md 「Verus」、README.md 「ビルドと実行」、root の CLAUDE.md の Commands
- 作業:
  - D-4 の `rust-toolchain.toml` と flake.nix の行
  - flake.nix のコメント: 9-10 行目は 「Verus の版を上げるときは、2つの URL、`rust-toolchain.toml` の `channel`、下の `stable` の `sha256` を一緒に変える」、41-42 行目は 「stable は版を `rust-toolchain.toml` から取り、devShell、`nix build`、Verus と、nix の外の rustup が同じ版を使う」 の中身にする
  - architecture.md の toolchain の箇条 (64 行目あたり): stable の版は `rust-toolchain.toml` に書き、flake がその `channel` を読み、nix の外では rustup が同じファイルを読む。devShell の cargo は rustup を通らないので、shell の中ではファイルは効かない
  - testing.md 「Verus」の最後の文: 版を上げるときの一覧を 「2つの input の URL、`rust-toolchain.toml` の `channel` と `flake.nix` の `stable` の `sha256`、`eml_rt` の vstd の版」 にし、`sha256` が古いと評価が hash の食い違いで止まることを足す
  - README.md 「ビルドと実行」: nix を使わないときは rustup が `rust-toolchain.toml` の版を入れる、と1文
  - CLAUDE.md の Commands: "stable is pinned in `rust-toolchain.toml` to the version Verus requires (1.98.1); the flake reads its channel and rustup reads the same file outside nix"
  - 試作: `s2-ci.patch` の `rust-toolchain.toml` と `flake.nix` の hunk
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - 新しいファイルを `git add` してから、次のループを親のコミットと、このコミットの両方で流し、出力が同じになる: `for a in devShells.aarch64-darwin.default devShells.aarch64-darwin.miri packages.aarch64-darwin.verus devShells.x86_64-linux.default devShells.x86_64-linux.miri; do nix eval --raw .#$a.drvPath; echo; done`
  - `nix develop -c cargo --version` → `cargo 1.98.1`
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → すべて passed
- テストの変更: なし

### T-11: CI のワークフローと docs

- 目的: 3つの OS と nix のジョブを流すワークフローを入れる (D-5、D-17)
- 対象: `.github/workflows/ci.yml` (新しいファイル)、testing.md、root の CLAUDE.md、crates/eml_rt/CLAUDE.md
- 作業:
  - `ci.yml` は `s2-ci.patch` の全文をそのまま使う。1行目のコメントは `# 流すものと理由は docs/implementation/testing.md の「CI」にある` である。`test` ジョブの `Install the pinned toolchain` の手順の直後には、D-7 の次の手順がある (patch に入っている)

    ```yaml
          # Windows の target は msvc だけである
          - name: Check the Windows host
            if: runner.os == 'Windows'
            run: |
              test "$(rustc -vV | sed -n 's/^host: //p')" = x86_64-pc-windows-msvc
    ```
  - testing.md 「方針」に箇条を足す: 「テストは macOS、Linux、Windows の CI でも流す。CI が流すものは「CI」にある」
  - testing.md の 「feature の組み合わせの確認」の後、「よく使うコマンド」の前に `## CI` を足す。中身は D-5 と D-17 の事実で、次の下書きを yomiyasu で整えて使う

    > `.github/workflows/ci.yml` は、main への push と pull request と手動の起動で、次の4つのジョブを流す。
    > - `test`: macOS (macos-15)、Linux (ubuntu-24.04)、Windows (windows-2025) のそれぞれで、`cargo nextest run`、`cargo clippy --all-targets -- -D warnings`、既定でない feature の clippy (`eml_cli` の3つと `eml_test_support` の4つ。「feature の組み合わせの確認」) を流す。toolchain は rustup が `rust-toolchain.toml` から入れ、`rustc -V` の版が `channel` と同じことを確かめる。Windows の target は `x86_64-pc-windows-msvc` だけで、ジョブは host がそれであることを確かめる。cargo-nextest はワークフローに書いた版を入れる
    > - `fmt`: Linux で `cargo fmt --check` を流す
    > - `verus`: Linux で、ふだんの devShell を通して `cargo verus verify -p eml_rt` と verusfmt の検査を流す
    > - `miri`: Linux で、Miri の devShell を通して、借用の規則ごとに1つのジョブを立て、`trusted::` と `miri::` を続けて流す
    >
    > CI が見るのは合否だけである。テストの時間の条件 (「性能のテスト」) と Miri の CPU 時間の予算は開発機で確かめ、runner の速さに合わせて緩めない。runner は開発機より遅いが、nextest がテストを止める 60 秒は1秒の条件より十分に長い。落ちたテストは流し直さない。合否が速さや順序で変わるテストは、流し直しで隠さずに直す。`#[ignore]` を付けたテストと `bench/` の計測は CI で流さない。
    >
    > アクションは commit の SHA で固定し、版の名前をコメントに書く。nix のストアはキャッシュしない。cargo の依存のビルドは `test` ジョブだけがキャッシュし、main の実行だけが保存する。ワークフローを変えたら `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` で検査する。
  - testing.md 「よく使うコマンド」: `cargo clippy --all-targets && cargo fmt` を `cargo clippy --all-targets -- -D warnings && cargo fmt` にし、`nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml   # CI のワークフローの検査` の行を足す。「Miri」と 「Verus」には CI の文を書かない (事実は 「CI」 の1か所に置く)
  - root の CLAUDE.md の Commands: 同じ clippy の行の変更と actionlint の行。Testing に箇条を1つ: "CI (`.github/workflows/ci.yml`) runs the tests, clippy and the feature-combo clippy on macOS, Linux and Windows, `cargo fmt --check` on Linux, and Verus, verusfmt and Miri on Linux through nix; it checks pass/fail only, never timing, and never retries (docs/implementation/testing.md の「CI」)."
  - crates/eml_rt/CLAUDE.md: `cargo clippy --all-targets` を `cargo clippy --all-targets -- -D warnings` にする
- 実装者に任せること: なし
- 条件: いつも。T-1〜T-10 の後に入れ、最初の CI の実行が仕上がった Windows の移植を試すようにする
- 完了の条件:
  - `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → 出力なし、exit 0
  - ワークフローのコマンドを開発機の devShell ですべて流し、すべて通る
    - `cargo nextest run --failure-output immediate-final` → すべて passed (`citations::*` を含む)
    - `cargo clippy --all-targets -- -D warnings` と7つの feature の clippy → それぞれ `Finished`、警告なし
    - `cargo fmt --check` → 出力なし
    - `cargo verus verify -p eml_rt` → 0 errors、7 warnings
    - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 出力なし
    - `MIRIFLAGS="-Zmiri-strict-provenance"` と `MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows"` のそれぞれで、`nix develop .#miri -c cargo miri test -p eml_rt trusted::` → 108 passed、`nix develop .#miri -c cargo miri test -p eml_vm --test integration miri::` → 10 passed
  - `test "$(rustc -V | cut -d' ' -f2)" = "$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)"` → exit 0
- テストの変更: なし

### T-12: 引用の検査が設定のファイルを読む

- 目的: 設定のファイルにある docs の引用を確かめる (D-18)
- 対象: `crates/eml_cli/tests/citations.rs`、testing.md 「文書の引用の検査」
- 作業: D-18 のとおりに足す。testing.md の、読むファイルを並べた文にこれらを足す。試作は `s2-ci.patch` の `citations.rs` の hunk
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → すべて passed
  - 落ちることの確かめ: `rust-toolchain.toml` の 「Verus」 をない見出しに替えると、`citations::every_cited_heading_exists` が落ちる。確かめたら戻す
- テストの変更: 既存のテストの読む範囲を広げる (D-18)

### T-13: CI を流し、初回の失敗を直す

- 目的: 7つの check をすべて通す
- 作業:
  1. 利用者に push の許可を求める。許可が出たら段の branch を push し、draft PR を作る。`gh pr checks --watch` か `gh run watch` で見る
  2. 落ちたジョブは、下の決めた手順で直し、同じ branch に直しのコミットを push する。新しい Windows の印か `cfg(unix)` は種類1の変更なので、計画を直して利用者の承認を取ってから入れる。nix のファイルとワークフローの直しは、テストを変えないので計画を直さなくてよい
  3. すべて通ったら止め、利用者に統合の仕方を聞く (T-14 の後に統合する)
- 初回の失敗の手順:
  1. `verus` が Linux の shell をビルドできないか、動かせない: 直す所は `nix/verus.nix` (verusfmt なら `nix/verusfmt.nix`)。autoPatchelf が依存 X を満たせなければ、X を持つ library を Linux の `buildInputs` に足す。rust_verify が `librustc_driver` か sysroot を見つけられなければ、wrapper の `LD_LIBRARY_PATH` が同じ `stable.rustc` の `${toolchain}/lib` を指すことと、`rustc --print sysroot` がそこを指すことを確かめる。z3 が起きなければ、1つ目と同じく libstdc++ を見る。直したら T-10 の drvPath のループで、darwin の drvPath が変わらないことを確かめる。nix のファイルを直しても Verus が x86_64-linux で動かなければ、止めて利用者に相談する (Linux で Verus を流すことは前提で、変えるのは計画の変更である)
  2. `miri` がテストの前に落ちる (nightly の取得、`cargo miri setup`、sysroot のビルド): 直す所は `flake.nix` の `nightly`。固定した fenix の rev の `complete` に x86_64-linux の miri がなければ `nix flake update fenix` を流し (flake.lock の変更)、開発機でも Miri を流し直す。Miri のテスト自体の失敗は本物の発見で、コードで直し、CI を変えて隠さない
  3. Windows のテストが、PATH で見つかったプログラムのせいで落ちる: job の bash は Git Bash で、PATH に MSYS の道具 (`sh`、`cat` など) がある。その1つのテストだけを `shell: pwsh` で流す使い捨てのコミットか、テストが起こしたプログラムを読んで原因を確かめる。合否が PATH の中身によるならテストが悪いので、D-1 に従って直す (ないプログラムは決してない名前にし、子は `eml` 自身にする)。確かめる動きが POSIX の道具を要るなら、T-5 の印の仲間で、種類1の変更として計画を直す。テストを通すために Windows の shell を替えない
  4. `Install the pinned toolchain` の手順が落ちる: `rustup toolchain install "$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)" --profile minimal -c clippy,rustfmt` に替える
  5. ジョブの時間切れ: そのジョブの `timeout-minutes` を、見た時間の約3倍にする。nextest の 60 s で止まるテストは、テストの欠陥で、時間切れを上げない
  6. Windows のテストの stderr に `warning: cannot install the Ctrl-C handler` が出る: T-4 の落とし穴の直し
  7. libm の最後のビット、スタックのあふれ、OS の誤りの文言: D-1 の直し
  8. ほかの失敗: 原因を調べ、テストの変更を伴うなら計画を直して承認を取る
- 実装者に任せること: 手順 8 の調べ方。直しの種類は 6 の決まりで決める
- 条件: いつも。push は利用者の確かめの後だけ
- 完了の条件:
  - `gh pr checks <PR>` → 7つの check (`test (ubuntu-24.04)`、`test (macos-15)`、`test (windows-2025)`、`fmt`、`verus`、`miri (stacked borrows)`、`miri (tree borrows)`) がすべて pass
  - 3つの OS の `Install the pinned toolchain` の log に `rustc 1.98.1` が出る。Windows の `Check the Windows host` が通る
  - `verus` の log に `1043 verified, 0 errors` が出る (数は T-6 の後の開発機と同じ)
  - Windows の `test` の log に `eml_cli::bin/eml interrupt::windows::tests::` の2つの PASS があり、`cannot install the Ctrl-C handler` がない

### T-14: 段を閉じる

- 目的: roadmap.md 「進め方」の閉じる手順で、docs を今の状態にそろえる
- 対象: spec、docs/implementation/benchmarks.md、status.md、architecture.md、roadmap.md、overview.md、README.md、CLAUDE.md、`docs/plans/2026-10-23-platform-ci.md` と付録のディレクトリ
- 作業 (roadmap.md 「進め方」の順で、1つのコミット `Close platform-ci`):
  1. spec に段の名前と 「今は〜、後で〜」 の予告が残っていないことを確かめる
  2. 計測: T-6 が `crates/eml_rt` を変えたので対象である。`bench/scripts.sh` を流し、範囲を外れた行があれば別のコミット `Resize bench rows` にする。`bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` をコードの最後のコミットの作業ツリーで流し、benchmarks.md 「今の数値」の表を入れ替える。測れなければ、表の入れ替えだけを直後の別のコミット `Measure platform-ci` にする
  3. status.md: 「既知の制限」に Windows の節を足す。待っている読みが Ctrl-C で止まらないこと (D-11)、`run (merge_stderr c)` が使えないこと (D-15)、子のプロセスの UI テストを Windows で流さないこと (D-1)
  4. architecture.md: T-3 と T-4 で直してある。`interrupt` モジュールと依存の表が合っていることを確かめる
  5. roadmap.md: 「段の列」から `platform-ci` の箇条を消す。`io-driver`、`worker-threads`、`native-aot` の 「前提」 から `platform-ci` を消す。次の項目を 「その後の項目」の合う分野に、「後に回した理由: …。」と 「見直す条件: …。」 の入れ子の箇条を添えて足す
     - std を通さない Windows のコンソールの標準入力の読み手 (Q-1)。後に回した理由: UTF-16 の変換、Ctrl-Z の扱い、Windows だけのテストが要り、この段の範囲を超える。見直す条件: `Stdin` の読みを待つ間の1回目の Ctrl-C が見えないことが問題になったとき
     - Windows で `eml` 自身を子にする移植できる Proc テストと、`inherited_stdout` の移植 (Q-2)。後に回した理由: Windows で通すテストがなかった。見直す条件: `io-driver` の段
     - Ctrl-C の handler の確かめと立てを不可分にする (Q-3)。後に回した理由: `Control` が前のビットを返す必要があり、`eml_rt` の変更になる。見直す条件: `TERMINATE` がビットごとの操作を `Control` に足すとき
  6. overview.md の確定した設計判断の表と、README.md 「文書の地図」の 「内容」 の欄を、変わった所だけ直す (testing.md に 「OS ごとのテスト」と 「CI」 が増えた)
  7. CLAUDE.md: T-5、T-10、T-11 で直してある。道しるべが合っていることを確かめる
  8. `docs/plans/2026-10-23-platform-ci.md` と `docs/plans/2026-10-23-platform-ci/` を消す。消す前に、5 の 「リポジトリに残す所」がすべて入っていることを確かめる
  9. コミットのメッセージに、docs で何をどこへ移したか、テストの変更の種類と理由 (8 の一覧)、計測の目立つ差、大きさを変えた行、後に回した項目の決めた日 (2026-10-23) を書く
- 実装者に任せること: 計測の 「所見」 を測り直すか消すかの判断 (roadmap.md 「進め方」の2の基準による)
- 条件: T-13 で7つの check が通った後
- 完了の条件:
  - `cargo nextest run` → すべて passed (`citations::*` を含む)
  - `git ls-files docs/plans` → 何も出ない
  - `rg -n 'platform-ci' docs CLAUDE.md crates` → 何も出ない
  - `rg -n '段の名前|今は.*後で' docs/spec` で、新しく足した予告がないことを目で確かめる

## 8. 進める順

```
T-1 ──> T-5 ──> T-6 ──> T-8
T-2 ──> T-9
T-3 ──> T-4 ──> T-7
T-10
          (T-1〜T-10 がすべて済む) ──> T-11 ──> T-12 ──> T-13 ──> T-14
```

- T-1、T-2、T-3、T-10 は互いに独立している。1つの branch に1つずつコミットするので、上の番号の順に進めてよい
- T-7 は T-4 の後に入れる (同じ `windows-sys` の行と vm.md の箇条を広げる)
- testing.md は T-2 (「CLI のテスト」の UTF-8 の文)、T-3 と T-4 (bin の単体テスト)、T-5 (新しい節と道しるべ)、T-6 (「期待値の表」)、T-11 (「方針」、「CI」、「よく使うコマンド」)、T-12 (「文書の引用の検査」) が触る。同じ文を2つの作業が書き換えることはない

### テストの変更の一覧

- 種類1 (成否の変更):
  - Windows で流さない UI テスト14本 (T-5 の一覧)
  - `cfg(unix)` のまま Windows で流さない12のテスト (記録だけで、コードは変えない): cli.rs の `an_interrupt_stops_a_running_program_with_130`、`repeated_interrupts_end_a_program_waiting_in_a_read_with_130`、`an_interrupt_stops_a_program_waiting_for_a_child_with_130`、`repeated_interrupts_end_a_program_waiting_for_the_standard_input_with_130`、`a_script_with_a_shebang_runs_directly`、terminal.rs の `interrupts::*` (3)、eml_rt の `heap::tests::host::std_host::{a_spawned_child_talks_through_pipes, a_spawned_child_runs_in_its_directory}`、eml_vm の `hosts::tests::{a_child_is_captured_after_it_ends, a_merged_error_joins_the_standard_output}`
  - `heap::tests::host::std_host::a_directory_opens_without_a_size` を消し、`a_directory_does_not_open` を足す (T-6)
- 種類2 (期待値の変更):
  - `crates/eml_rt/src/heap/tests/expected.txt` の2行、`read_directory.em` の1行目と snapshot (T-6。ディレクトリを開かない)
  - `commands__load.snap` (T-8。OS の文言を除く)
  - `api.rs` の2つの assert を Windows でも確かめる (T-7)
- 種類3 (機械的な追随): T-2 の移植
- 種類の外で、この計画で名指しするもの:
  - eml_cli の bin の `test = false` を外す (T-3。testing.md 「crate の中の置き方」の決まりの変更)
  - 引用の検査の読む範囲を広げる (T-12)
- 新しいテスト: `interrupt::tests::*` の4つ、`interrupt::windows::tests::*` の2つ (Windows だけ)、`ui::the_windows_mark_is_the_first_line`

## 9. 確かめの計画

- 基点 (最初の変更の前): 段の branch の worktree で次を流して記録する
  - `cargo nextest run` → 3507 passed、18 skipped
  - `cargo clippy --workspace --all-targets -- -D warnings` → `Finished`、警告なし
  - `xclippy.sh <worktree>` → `x86_64-pc-windows-msvc` で `script.rs:77` の `unused_mut` の誤りで止まる (既知。T-2 で直す)
- 全体の確かめ (T-12 の後、開発機で):
  - `cargo nextest run` → すべて passed。数は 3507 + 新しいテスト (`interrupt::tests` 4、`ui::the_windows_mark_is_the_first_line` 1、`a_directory_does_not_open` 1) − 消したテスト (`a_directory_opens_without_a_size` 1) = 3512 passed、18 skipped を目安にする。違えば差を説明できることを確かめる
  - `cargo clippy --workspace --all-targets -- -D warnings` と7つの feature の clippy → `Finished`、警告なし
  - `xclippy.sh <worktree>` → `Finished` が2回
  - `cargo fmt --check` → 出力なし
  - `cargo verus verify -p eml_rt` → 0 errors、7 warnings。verusfmt の検査 → 出力なし
  - T-11 の Miri の4つのコマンド → 108 passed と 10 passed
  - `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → exit 0
  - CI: T-13 の完了の条件
- 利用者が手で確かめること:
  - T-13 の push と draft PR の作成の許可 (完了の条件の2つ目)
  - 任意。段を閉じる条件ではない。Windows のコンソールがあれば次を確かめ、結果をコミットのメッセージに書く: 無限ループの `eml run` が Ctrl-C で `interrupted` を出して 130 で終わる、`Stdin.read_line` の間の2回目の Ctrl-C で 130 で終わる、REPL で編集中の Ctrl-C が行を捨て、Ctrl-Break で REPL が終わる

## 10. 危険と未決の項目

| ID | 項目 | 及ぶ所 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | std を通さない Windows のコンソールの読み手 | D-11 | 必要になったときの段で | 入れない。T-14 で roadmap に送る |
| Q-2 | Windows の Proc の移植できるテストと `inherited_stdout` の移植 | D-1、D-15 | `io-driver` の段 | 入れない。T-14 で roadmap に送る |
| Q-3 | handler の確かめと立ての競合 | D-10 | `TERMINATE` の段 | 受け入れて vm.md に書く。T-14 で roadmap に送る |
| Q-4 | ネイティブの印の書き方 (`// native: skip`) | D-2 | ネイティブの段 | この段では docs/future を変えない |
| Q-5 | `nix build`、Dependabot、branch protection | D-5 | `native-aot` の段 (`nix build`) | どれも入れない。利用者は main を手で統合して push するので、branch protection の必須の check はその流れを止める |
| Q-6 | runner の実時間 (推測で、test 5〜20 分、verus 約10分、miri 約15分) | D-17 | 最初の通った実行の後 | 時間切れは 60/10/30/45 のまま。見た時間の約3倍に直すのは、ワークフローだけの変更 (T-13 の手順5) |
| Q-7 | Ctrl-Break で終わったときの `eml run` の終了コード | D-10 | 必要になったとき | OS の既定のまま。docs に値を書かない |
| Q-8 | x86_64-linux の Verus の起動 | T-13 | 最初の CI の実行 | T-13 の手順1。直せなければ利用者に相談 |

## 付録: レビューで変わった主なこと

蒸し返さないための記録である。

- `inherited_stdout` の移植は外した。Windows でそれを通すテストがないため (Q-2)。`physical_memory` の移植は残した。`api.rs` が Windows の CI で通すため
- ディレクトリは、印でなく、どの OS でも開かないことにした。確かめてから開く形は費用と競合があるので、開いた後の `fstat` と、開けなかった後だけの `metadata` にした (D-12)
- トランスクリプトの入力は消さず、`not_utf8.txt` に替えた。消すと 「失敗した `:load` の後も定義が残る」 確かめを失うため (D-13)
- 入口のパスの `\` は Windows で落ちると読めたので、T-9 を足した
- 印は shebang でない最初の行にした。後のスクリプトのテストが shebang と印を両方持てるようにするため (D-2)
- Windows の handler の本体は入力を引数で受け、テストは static に触れない。2回目の Ctrl-C も `exit` の closure で確かめる (D-6)
- std はコンソールの読みを Ctrl-C で読み直す (OS の制限ではない)。spec はそう書く (D-11)
- main の concurrency はコミットごとの組にした。1つの組で待つ実行は1つだけで、新しい実行が古い待ちを取り消すため (D-17)
- install-action に token を渡す案は採らなかった。`fallback: none` では使わないため (D-17)
- 引用の検査の範囲を広げる変更は、テストの変更の種類の外として名指しした (D-18)
