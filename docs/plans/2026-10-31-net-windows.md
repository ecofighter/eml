# net-windows: Windows の IOCP の backend にソケットを載せる

- 作成: 2026-10-31
- 基点: `claude/happy-ptolemy-u2f1ko` @ `00d019e` (3節と行番号はこの状態を指す)
- 状態: 計画は確定し、ユーザーが承認した (Q-1 と Q-3 は決定済み)。ただし Windows の振る舞いは T-1 の試しが CI で通るまで仮定で、「止める」の扱いの試しが落ちたら計画を見直す (10節)
- 付録: `docs/plans/2026-10-31-net-windows/` に試作の diff を置く。どれも型検査と Linux のテストだけを通したもので、Windows では動かしていない。コメントは下書きなので、そのまま写さない
  - `probes-prototype.diff`: T-1 の試し (main にそのまま当てられる)
  - `core-prototype.diff`: T-2 の核の変更と核のテスト (main にそのまま当てられる。レビューの試しのテスト `r1_probe_*` と `s1_*` を含む。名前は 5.1 と T-2 のものに直す)
  - `backend-prototype.diff`: T-1 と T-3 を合わせたもの (main に当てる形なので `Hold::Buffer` のまま。T-2 の後に当て直して分ける)
  - `tests-prototype.diff`: T-4 (T-3 に入る2件のテストの削除と Cargo.toml の行を含む。T-4 ではそれを除く)
  - `terminate-prototype.diff`: T-5

## 1. 目的と完了の条件

- 目的: roadmap の `net-windows` の3つを入れる。自前の IOCP の backend にソケットを載せること、unix と同じ本物のネットのテストの組を Windows の CI で流すこと、`Std.Signal` の `Terminate` を Windows のコンソールの事象に写すこと
- 完了の条件: 9節の全体の確認がすべて通ること。要点は次のとおり
  - Windows の CI で、`eml_io` の `real::` が 47 件、`eml_vm` の `api::` が 29 件、`eml_cli` の `tls::` が 1 件通り、SLOW がない
  - `sys/iocp.rs` の試し 15 件と backend のテスト 11 件が Windows の CI で通る
  - `eml run` が `CTRL_CLOSE_EVENT` と `CTRL_SHUTDOWN_EVENT` を `Terminate` にし、その単体テスト 6 件が Windows の CI で通る
  - 段を閉じるコミット (`Close net-windows`) が roadmap の手順どおりに入る

## 2. 範囲

- 含む
  - 核と backend の境界のうち、完了型の値 (`Hold`、`Cancel`、`Arrival`、`Start`) を確定させ、暫定の扱いをやめる
  - 壊れた driver と driver を落とすときの規則の整理 (D-3)
  - IOCP の backend のソケット (`listen`、`socket`、`shutdown_write`、`set_nodelay`、`close`、`start`、`cancel`、`wait`)
  - Windows の CI で確かめる、文書にない OS の振る舞いの試し
  - `Backend::SOCKETS` と、核の `Unsupported` の道を消すこと
  - 本物のソケットのテストを Windows で流すこと
  - コンソールの事象から `Terminate` への写し
  - spec、実装の文書、CLAUDE.md の更新と、段を閉じる作業
- 含まない
  - io_uring の backend (roadmap の `io-uring`)。ただし D-1 から D-5 はその段も守る
  - Windows の非同期のファイルとパイプ (`async-files`)
  - `CTRL_LOGOFF_EVENT` の写し (D-14)
  - Windows の Miri と AddressSanitizer。どちらの道具も Windows を対象にしない。backend の `unsafe` は、テストとレビューだけで確かめる

## 3. 背景

- 核 `crates/eml_io/src/driver/mod.rs` は OS を呼ばない。backend は trait `Backend` (`driver/backend.rs`) の後ろにあり、Linux は epoll、Apple は kqueue、Windows は IOCP である (`sys/mod.rs`)
- 完了型の値 `Hold::Buffer`、`Cancel::Pending`、`Arrival::Done`、`Arrival::Cancelled` は暫定と書かれていて、今は核のテストの偽の backend (`driver/tests/fake.rs`) だけが使う (`backend.rs:4-5`、`:48-49`、`:94-96`)
- 核が欄の `hold` を読むのは `Drop` の1か所だけである (`driver/mod.rs:909`)。`quiesce` は `hold` を見ず、取り消した欄を最も古い `cancel_sent` から `CANCEL_LIMIT` (1 秒) まで待つ
- 核の `Drop` は今、`!slot.cancelled` の欄にだけ OS の取り消しを出す (`mod.rs:920-924`)。壊れた driver の `Driver::cancel` は印を立てるだけで OS を呼ばない (`mod.rs:741-744`)。そのため、`quiesce` で取り消しが1回断られると、残りの欄には OS の取り消しが出ないまま `Drop` が 1 秒待って abort する。偽の backend の試しで再現した
- 待ちの失敗で壊れた driver (`By::Wait`) は、`Hold::Buffer` の欄が残っていれば `Drop` ですぐ abort する (`mod.rs:913-918`)
- Windows の backend `sys/iocp.rs` は `SOCKETS = false` で、`Sock` は値のない enum である。ポートで起こしと時間切れだけを待つ。核は `SOCKETS` が偽ならソケットの操作を `Unsupported` で終える
- `crates/eml_io/src/trust.rs` は `src/` だけを走査する。`unsafe` を書けるファイルを `UNSAFE_FILES` で固定し、readiness の backend (`sys/unix.rs`、`sys/epoll.rs`、`sys/kqueue.rs`) が完了型の値を名指さないことを確かめる (`readiness_backends_report_only_retries`)。`tests/` は対象外である
- `crates/eml_io/Cargo.toml` の windows-sys の feature は `Win32_Foundation`、`Win32_System_IO`、`Win32_System_Threading` だけである
- テストの数 (基点):
  - `crates/eml_io/tests/real.rs`: `#[test]` が 51 件。`cfg(unix)` が 35 件、`cfg(windows)` が 1 件 (`listen_accept_and_connect_are_unsupported`)、cfg なしが 15 件
  - `crates/eml_vm/tests/api.rs`: 31 件。本物のソケットの 9 件が `cfg(unix)`。`a_real_listen_is_not_available` が Windows だけ
  - `crates/eml_cli/tests/tls.rs`: 1 件。`tests/main.rs:14` で `cfg(all(unix, feature = "tls"))`
- Windows のコンソールの handler (`crates/eml_cli/src/interrupt/windows.rs`) は今 `CTRL_C_EVENT` だけを扱い、ほかの事象には FALSE を返す。`Terminate` を立てる Windows の道はない
- Windows の事実 (Microsoft の文書と、libuv と Go の実装):
  - `CTRL_CLOSE_EVENT`、`CTRL_LOGOFF_EVENT`、`CTRL_SHUTDOWN_EVENT` では、handler が TRUE と FALSE のどちらを返しても、返ったところでプロセスが終わる。待ち時間の既定は、閉じるが 5 秒、シャットダウンが 5 秒、サービスでは 20 秒である
  - `GenerateConsoleCtrlEvent` が送れるのは `CTRL_C_EVENT` と `CTRL_BREAK_EVENT` だけである
  - user32 か gdi32 を読み込んだプロセスには、Windows 7 から 10 まで、ログオフとシャットダウンの事象が届かない。`eml` は rustyline 19 から `clipboard-win` 5.4.3 を通して user32 を link する (ソースで確認。link で残るかは未確認)
  - 長さ 0 の `WSARecv` の振る舞い、`OVERLAPPED` なしの呼び出しが packet を積まないこと、`accept` と `AcceptEx` の併用は、Microsoft の文書に書かれていない。libuv と Go は前の2つに頼っている
  - `CancelIoEx` で取り消した保留の操作にも packet が届く。`ERROR_NOT_FOUND` は取り消す操作がないことを表す。`closesocket` も保留の操作を取り消し、packet は届く。`OVERLAPPED` は操作が終わるまで放してはならない
  - windows-sys 0.61 には `fn ConnectEx` がなく、`LPFN_CONNECTEX` と `WSAID_CONNECTEX` だけがある。`TCP_INITIAL_RTO_PARAMETERS.MaxSynRetransmissions` は `u8` で、Go は 254 を渡す
- CI (`.github/workflows/ci.yml`) は `pull_request`、main への `push`、`workflow_dispatch` で走り、作業のブランチへの push では走らない。Windows の job (windows-2025、60 分) は `cargo nextest run` と tls_off の run、clippy、feature の組の clippy を流す。nextest は再試行しない
- このコンテナでは Windows のコードを動かせない。型検査は `.#cross` の clippy でできる (次の形で確認済み。既定の feature では aws-lc-sys の C の組み立てで落ちる):
  ```sh
  nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings
  ```

## 4. 前提の確認

| 要るもの | 用途 | 確かめ方 | ないとき |
|---|---|---|---|
| nix の devShell と `.#cross`、`.#miri` | 全タスク | `nix develop .#cross -c true` と `nix develop .#miri -c true` → 終了コード 0 | `nix/prefetch-inputs.sh` を流してから再試行する。それでもなければ止めて聞く |
| GitHub の `ecofighter/eml` への push と Actions の起動 | T-1 から T-5 の Windows の確認 | GitHub MCP の `actions_list` で `ci.yml` が見え、`actions_run_trigger` で `workflow_dispatch` を起こせる | 止めて、ユーザーに GitHub の接続を頼む |
| 段のブランチへの force-push の許可 | 緑になるまでタスクのコミットを直して出し直す (D-20) | ユーザーの承認 (Q-1) | 承認がなければ、赤の CI は修正のコミットを重ねて直す。履歴に赤のコミットが残ることをメッセージに書く |
| macOS の機械 | 段を閉じる計測 (T-6) | ユーザーに聞く | T-6 では表を入れ替えず、直後の別のコミット (`Measure net-windows`) をユーザーに頼む (Q-3) |

## 5. 決めたこと

### 5.1 長期の指針 (変えるときは見直しから)

実装でこれらとぶつかったら、止めて報告する。io_uring の段もこの境界に載る。

#### D-1: 完了型の値の最終形

- 決定: 暫定の扱いをやめ、次の形にする。`Step`、`Outcome`、`Backend` の関数の形は変えない。`Hold::Buffer` は `Hold::Memory` に名前を変える
  ```rust
  /// `Pending` の間に OS がこちらの記憶に書きうるか。核は driver を落とすときだけ読む。
  pub(crate) enum Hold {
      /// `start` が戻った後、OS はこちらの記憶に触れない。落とすときは待たずに忘れてよい。忘れた欄に遅れて届く
      /// event は backend が捨てる (readiness の backend は世代で捨てる)。
      None,
      /// 貸された buffer か backend の記憶 (IOCP の `OVERLAPPED` の箱、AcceptEx の番地の buffer、ConnectEx の
      /// sockaddr) に、完了まで OS が書きうる。核は終わりの event を読むまで欄を放さず、落とすときは 1 秒まで待って
      /// から abort する。
      Memory,
  }
  pub(crate) enum Start<S> {
      /// 歩はその場で終わった。この歩を名指す event は届かない。
      Done(io::Result<Outcome<S>>),
      /// 歩は OS に出た。`cancel` が `Done` を返さない限り、この鍵を名指す event がちょうど1つ届く。
      Pending(Hold),
  }
  pub(crate) enum Cancel {
      /// backend は鍵と待ち手を忘れ、OS はこちらの記憶に触れず、event は届かない。核はその場で `cancelled` の完了を
      /// 積む。readiness の backend はいつもこれを返す。完了型の backend がこれを返すのは鍵の操作が OS に残って
      /// いないときだけで、保留の鍵には起こらない (起これば契約の破れで、backend は debug_assert の上でこれを返す)。
      Done,
      /// OS に取り消しを頼んだか、終わりがすでに列にある。終わりは `wait` の `Event` で届く: `Cancelled`、取り消しが
      /// 間に合わなかった `Done(結果)`、または `Retry`。
      Pending,
  }
  pub(crate) enum Arrival<S> {
      /// 同じ `Step` をもう1度 `start` せよ。readiness の知らせと、IOCP の長さ 0 の `WSARecv` の成功の packet。
      /// 余分に届いても害がない。
      Retry,
      /// OS がこの歩を終えた結果。取り消しを頼んだ後に届くこともある。長さ 0 の `WSARecv` の誤りの packet もこれで届く。
      Done(io::Result<Outcome<S>>),
      /// 核が `cancel` で頼んだ取り消しで終わった。結果はない。`cancel` が `Pending` を返した鍵にだけ届く。
      Cancelled,
  }
  ```
  trait の doc に足す規則: `wait` は event を積んだ後に `Err` を返さない。readiness の backend は `Retry`、`Pending(Hold::None)`、`Cancel::Done` だけを使い、完了型の backend はすべての値を使う
- 理由: 核が `hold` を読むのは落とすときだけで、貸した buffer と backend の箱の扱いは同じ (1 秒待って abort) なので、3つ目の値は区別だけで規則を持たない。backend の `Drop` で待つ形は、時計、取り消し、abort を backend ごとに写すことになる。`Buffer` の名前のままでは、buffer を貸さない長さ 0 の `WSARecv` に `Hold::None` を返す誤りを招く
- 退けた案: `Hold` に3つ目の値を足す (核の扱いが同じ)。IOCP の backend の `Drop` で packet を待つ (時計と abort が二重になり、待ちが壊れたときを扱えない)。`Backend` に `const HOLDS: bool` を置く (io_uring は何も持たない `POLL_ADD` と buffer を持つ `RECV` を混ぜうる)。取り消しを `Done(Err(cancelled))` で届ける (`settle` が `cancelled: false` にしてしまう)
- 及ぶ範囲: `driver/backend.rs`、`driver/mod.rs`、すべての backend
- 見直す条件: io_uring の段で、`POLL_ADD` の欄に遅れて届く CQE を世代で捨てられないと分かったとき
- 適合の確認:
  - G-1: `Hold` は `None` と `Memory` の2つだけで、OS が貸し手か backend の記憶に書きうる保留にはどれも `Memory` を返す (IOCP では長さ 0 の `WSARecv` を含むすべての重なった操作)。確かめ方: `grep -c 'Hold::None' crates/eml_io/src/sys/iocp.rs` → 0
  - G-3: backend は、どの event もその歩を名指さないときだけ `Start::Done` を返し、packet か CQE が届くときは、同期に成功した重なった呼び出しも含めて `Start::Pending` を返す。`wait` は event を積んだ後に `Err` を返さない。確かめ方: IOCP の `wait` の先頭の `debug_assert_eq!(boxes.len(), waiters.len())`、backend のテスト `an_overlapped_send_through_the_backend_ends_with_its_size`、試し P7
  - G-9: 完了型の値 (`Arrival::Done`、`Arrival::Cancelled`、`Hold::Memory`、`Cancel::Pending`) を名指すのは `driver/backend.rs`、`driver/mod.rs`、`driver/tests/`、`COMPLETION_BACKENDS` のファイルだけである。確かめ方: trust.rs の T2
  - G-11: `crates/eml_io/src`、`docs/spec`、`docs/implementation` に「暫定」の扱いが残らない。確かめ方: T-2 の grep
- リポジトリに残す場所: `backend.rs` の doc コメント、`docs/spec/vm.md` の「時計と driver」、`docs/implementation/architecture.md` の「`eml_io` の内部」、`trust.rs` の規則 (T-2 が書く)

#### D-2: 核の責務と、核と backend の約束

- 決定: 核は続きの規則、`cancelled` の印、壊れた状態、`quiesce` と `Drop` の待ちを持ち、`hold` は `Drop` でだけ読む。OS に取り消しを出したかどうかは `cancel_sent` で表し、`Hold` には入れない。約束は次のとおり
  - 核は、保留の歩があるソケットに `backend.close(sock)` を呼ばない。保留の鍵を使い回さない。1つの欄に、`quiesce` か `cancel` から1回と `Drop` から1回を超えて取り消しを出さない。`Start::Pending` の後は、`cancel` が `Done` を返さない限り、event がその欄を名指すまで欄を残す
  - backend は、`Start::Done` のときは event を送らない。同期に成功した重なった呼び出しも `Pending` にする。`cancel` は `CancelIoEx` の成功と `ERROR_NOT_FOUND` で `Pending`、ほかの失敗で `Err` を返す。`Cancelled` は `Pending` の取り消しの印がある箱にだけ届け、印のない箱の中断は `Done(Err)` にする。`wait` は event を積んだ後に `Err` を返さない。箱が残ったまま backend が落ちるときは abort する
- 理由: `cancel_sent` はすでにあり、C-1 の不具合 (3節) を直すのに要る状態はこれで足りる
- 適合の確認:
  - G-2: 核が `hold` を読むのは `Drop` の中だけである。確かめ方: trust.rs の T3
  - G-6: `Cancelled` は `Pending` の取り消しの印がある箱にだけ届く。印のない箱の中断は `Done(Err)` である。確かめ方: 核の `arrive` の `Cancelled` の腕の `debug_assert!(slot.cancelled)`、核のテスト10
  - G-7: 受けのソケットは箱の中にあり、`Outcome::Accepted` でだけ箱を出る。接続のソケットは packet の後でだけ核が閉じる。核は保留の歩があるソケットに `backend.close` を呼ばない。確かめ方: 核のテスト5と既存の `closing_a_handle_with_a_pending_op_closes_after_the_op`、IOCP の `close` の `debug_assert!`
- リポジトリに残す場所: `backend.rs` の trait の doc、vm.md の「時計と driver」 (T-2)

#### D-3: driver を落とすときの規則

- 決定: 健全な driver も壊れた driver も、壊れた理由によらず同じ道で落とす
  1. `Hold::None` の欄は待たずに忘れる (接続のソケットは閉じる)
  2. `cancel_sent` が `None` の欄すべてに、`backend.cancel` を1回ずつ出す (`Pending` か `Err` なら印を立て、`Done` なら欄を消す)
  3. 欄がなくなるまで `turn` を回す。`HELD_LIMIT` (1 秒) を過ぎるか `turn` が `Err` を返したら abort する

  待ちの失敗で壊れたときの「すぐ abort する」特別扱いと `enum By` は消す。`Broken` は `{ kind, msg }` だけを持つ。`quiesce` は今までどおり、`hold` によらず取り消したすべての欄を `CANCEL_LIMIT` まで待つ
- 理由: roadmap の前提「壊れた driver を落とすときは、取り消しを出して 1 秒まで終わりを確かめ、終わらなければ process を abort する」を文字どおりに当てる。待ちを1回試し直しても、壊れたままなら1回の失敗の後で abort するだけで、一時の失敗なら回復できる
- 退けた案: 壊れた `cancel` の枝で印を立てない (`Drop` で1か所にまとめるほうが変更が小さい)。`quiesce` の取り消しを最初に断られた所で止める (残りの欄が同じ問題を持つ)
- 適合の確認:
  - G-8: 落とすときは、健全でも壊れていても、`Hold::None` の欄を待たずに忘れ、`cancel_sent == None` の欄に1回ずつ `backend.cancel` を出し、`HELD_LIMIT` まで待ち、欄が残るか待ちが失敗したら abort する。`quiesce` は `hold` によらず `CANCEL_LIMIT` まで待つ。確かめ方: 核のテスト6、7、8、11と既存の abort のテスト2件。`grep -n 'enum By' crates/eml_io/src/driver/mod.rs` → 出力なし
- リポジトリに残す場所: vm.md の「時計と driver」の `Drop` の箇条と、eml_rt の `IoDriver` の doc (T-2)

#### D-4: `OVERLAPPED` の箱の所有

- 決定: 箱は backend が持ち、核は見ない。OS に出した操作1つに箱1つで、鍵1つと (ソケット、向き) 1つに高々1つである。重なった呼び出しの前に作り、`Box::into_raw` から `Box::from_raw` までは OS が持つ。取り戻すのは、その箱を名指す packet を `wait` で取り出したときと、呼び出しが同期に失敗したとき (packet は来ない) の2か所だけである。`cancel`、`close`、`quiesce`、核の `Drop` は箱を放さない。`AcceptEx` の受けのソケットは箱のフィールドで、成功の packet で `Outcome::Accepted` として箱を出る。それ以外の終わり (同期の失敗、`Done(Err)`、`Cancelled`) では箱を落とすときにソケットも閉じる。`ConnectEx` のソケットは `backend.socket` で作った時点から核の `Kind::Connect` が持ち、箱は sockaddr だけを持つ
- 理由: `CancelIoEx` の成功、`ERROR_NOT_FOUND`、`closesocket`、取り消しが間に合わなかった完了のどれでも、場合分けなしに正しいのはこの規則だけである
- 退けた案: packet の識別に世代を入れる (生きた箱の番地は一意で、要らない)。`ERROR_NOT_FOUND` で `Cancel::Done` を返す (packet は列にあり、箱は OS が持ったまま)
- 適合の確認:
  - G-4: 箱を取り戻すのは、それを名指す packet を取り出したときと同期の失敗のときだけで、`cancel`、`close`、`quiesce`、核の `Drop` は放さない。箱が残ったまま backend が落ちるときは abort する。`lpOverlapped` が null の packet と、ソケットの key でない packet は、放さずに捨てる。確かめ方: trust.rs の T4 と T5。`grep -ow from_raw crates/eml_io/src/sys/iocp.rs | wc -l` → 2、`grep -ow into_raw … | wc -l` → 1。`impl Drop for Iocp` が abort を持つ
  - G-5: `cancel` は `CancelIoEx` の成功と `ERROR_NOT_FOUND` で `Pending`、ほかの失敗で `Err`、箱のない鍵では契約の破れとして `debug_assert!` の上で `Done` を返す。確かめ方: backend のテスト2、3、4。核のテスト7
- リポジトリに残す場所: architecture.md の `sys/iocp.rs` の箇条と vm.md の「時計と driver」 (T-3)

#### D-5: 試作の印とコミットの順

- 決定: コミットの順は T-1 (試し)、T-2 (境界と核)、T-3 (backend)、T-4 (テストの移行) で、T-5 (`Terminate`) はその後に置く。どのコミットも、すべての CI の対象で組み立てと `clippy -D warnings` を通す
- 適合の確認:
  - G-10: T-2 は `backend.rs` と `sys/iocp.rs` の `cfg_attr(…, expect(dead_code))` の印を1つも変えない。T-3 が D-11 のとおりに変え、`.#cross` の clippy を通す。確かめ方: `git show <T-2> -- crates/eml_io/src/driver/backend.rs | grep '^[-+].*cfg_attr'` → 出力なし
- リポジトリに残す場所: 不要 (コミットの並びで閉じる)

### 5.2 短期から中期の設計

理由があれば実装中に直してよい。直したらコミットのメッセージに残す。

#### D-10: Windows の試しは `sys/iocp.rs` の単体テストに置き、恒久に残す

- 置き場所は `crates/eml_io/src/sys/iocp.rs` の末尾の `#[cfg(test)] mod tests` である。testing.md の「テストの置き場所」の4行目が backend のテストの場所で、G-3 は単体テストを求める。T-3 から backend の関数を共有できる。`sys/iocp.rs` はすでに `UNSAFE_FILES` にあるので、T-1 は trust.rs を変えない
- 試しの道具 `Rig`: 自前のポートと `Box<[UnsafeCell<OVERLAPPED>; 8]>` を持ち、番号で貸す。packet を取り出すまで貸したままにし、貸したまま落ちるときは `process::abort()` する (OS が書きうる記憶を放さないため。nextest はテストごとに別のプロセスで流す)
  - `associate(s)`: `CreateIoCompletionPort(s, port, SOCK=1, 0)` が null でないこと
  - `packet_of(i)`: 1 秒以内に、key `SOCK` で番号 `i` を名指す packet が来ること
  - `only_marks()`: 印の packet (key 2) を積み、1 秒以内に次の packet がその印であり、続く 0 ms の待ちが空であること。packet がないと強く言えるのは、戻る前に終わった呼び出し (P6、P7、P4 の同期の部分) についてだけで、ほかでは確かめの補助にとどまる
  - Winsock は局所の `Once` で `WSAStartup(0x202)` を呼ぶ。packet の状態は `WSAGetOverlappedResult(s, ov, &bytes, FALSE, &flags)` で読む
- 時間: 待ちはどれも 1 秒で切り、眠らない。経過時間は assert しない。通る試しはループバックで数ミリ秒で終わる。落ちる試しは待ちの合計で最大 3 秒 (P9) かかる
- 拒まれるポートは `refusing(ip)` で作る。`ip:0` に bind して listen しない生のソケットを、テストの間持ち続ける。並行するテストにポートを取られない
- 15 件の試し。「対」は std のループバックの接続で、サーバの端を `set_nonblocking(true)` にしてポートに結び付けたもの。「長さ 0 の読み」は `WSABUF{len: 0, buf: null}` 1つの `WSARecv` である。「止める」は段を止めてユーザーに戻す。「その場で」は前もって認めた代わりの形で、実装者が当てて T-1 のコミットのメッセージに書く

| # | テストの名前 | 確かめること (表示するもの) | 決めること | 落ちたとき |
|---|---|---|---|---|
| P1 | `a_zero_read_waits_for_data_and_leaves_it` | 空の対で長さ 0 の読み → `SOCKET_ERROR` と `WSA_IO_PENDING`。`only_marks`。相手が `ab` を書く → packet は `Ok(0)` で 0 バイト。`recv` → `ab`、次の `recv` → `WSAEWOULDBLOCK` | `Peek` と `Recv` の待ち。読みの buffer を貸さないこと | 止める。代わりの設計は、箱の中の1バイトへの `MSG_PEEK` の重なった `WSARecv` |
| P2 | `a_zero_read_completes_on_a_closed_peer` | 保留の長さ 0 の読み。相手が `shutdown(Write)` → packet `Ok(0)`。`recv` → 0 | EOF を `Retry` で届けること | 止める (P1 と同じ) |
| P3 | `a_zero_read_ends_on_a_reset` | 保留の長さ 0 の読み。相手が `SO_LINGER{1,0}` で落ちる。packet の誤りか次の `recv` の誤りの番号が 10054、10053、64 のどれか | D-12 のリセットの行 | その場で: 見えた番号を `stream_error` に足す |
| P4 | `a_zero_read_on_buffered_data_queues_a_packet_that_a_cancel_does_not_find` | 相手が書く。`MSG_PEEK` のループ (1 秒まで) でデータを待つ。長さ 0 の読み → 0 (同期の成功)。`CancelIoEx` → `ERROR_NOT_FOUND`。packet `Ok(0)` が印より先に来る。`recv` → 2 バイト | G-3 (`WSARecv`)、G-5 の `ERROR_NOT_FOUND` | packet が来ない: 止める。`Ok(0)` の assert だけが落ちる: その場で、「同期か保留」に緩める |
| P5 | `a_cancelled_zero_read_ends_aborted` | 保留の長さ 0 の読み。`CancelIoEx` が成功し、packet は `Err(995)` | G-5、G-6 | その場で: 見えた番号を「中断」として扱う |
| P6 | `calls_without_an_overlapped_queue_no_packet` | 結び付けた FIONBIO のリスナーで `accept` → WOULDBLOCK、その後に成功 (ループに上限)。受けたソケットに FIONBIO、`CreateIoCompletionPort` が成功 (結び付けは受け継がれない)。続けて `recv` の WOULDBLOCK、データの `recv`、`send`、`TCP_NODELAY`、`getsockname`、`getpeername`、重ならない `WSAIoctl`、`shutdown(SD_SEND)`。`only_marks`。相手が `cd` を読む。ループバックの往復とファイアウォールの確かめも兼ねる | 先に試す形。`accept` の後の結び付け | `lpOverlapped` が null の packet が余分に来る: その場で、`wait` の `(SOCKET_KEY, null)` の `debug_assert` を外して捨てる。ほかの落ち方: 止める |
| P7 | `an_overlapped_send_that_completes_at_once_still_queues_a_packet` | 空の対で 4 バイトの `WSASend` → 0。その packet (4、`Ok(4)`) が印より先に来る。相手が `abcd` を読む | G-3。成功時に packet を省くモードを使わないこと | 止める |
| P8 | `an_overlapped_send_on_a_full_socket_pends_and_cancels` | WOULDBLOCK まで 64 KiB を `send` (256 MiB まで)。8 MiB の重なった `WSASend` は `Ok` (WOULDBLOCK でない)。相手が 1 MiB 読む。`CancelIoEx` は成功か `ERROR_NOT_FOUND`。packet はちょうど1つで、`Ok(n ≤ 8 MiB)` か `Err(995)` (どちらかを表示) | 塞がれたときだけ `WSASend` を出すこと。書きの取り消し。T-4 の F-19 | WOULDBLOCK になる: 止める (代わりは Go の形。FIONBIO にせず重なった呼び出しだけを使う) |
| P9 | `accept_and_accept_ex_share_a_listener` | (a) `accept` → WOULDBLOCK。(b) `AcceptEx` (新しいソケット、受けの長さ 0、番地の長さ 144 と 144、buffer 288) → `WSA_IO_PENDING`。(c) 接続1 → packet `Ok`。`SO_UPDATE_ACCEPT_CONTEXT`、FIONBIO、結び付けが成功し、`getpeername` が接続1を返す。(d) 接続2と3を `accept` が順に受け、次は WOULDBLOCK。(e) `AcceptEx` より先に来た接続4を `AcceptEx` が受ける | 先に `accept`、塞がれたら `AcceptEx`。その間の競合 | (e) が落ちる: その場で、いつも `AcceptEx` にする。(c) が落ちる: 止める |
| P10 | `a_cancelled_accept_ex_ends_aborted` | 保留の `AcceptEx`。`CancelIoEx(listener, ov)` が成功し、packet は `Err(995)`。その後もリスナーは受けられる | 受けの取り消し | その場で (P5 と同じ) |
| P11 | `a_refused_loopback_connect_ex_fails_fast` | `127.0.0.1` と `::1` のそれぞれで、any (ポート 0) に bind、`SIO_TCP_INITIAL_RTO{Rtt: 0, MaxSynRetransmissions: 254}` → 0、FIONBIO、結び付け、`refusing(ip)` へ `ConnectEx`。同期か 1 秒の待ちの中の packet で、番号が {10061, 1225} のどれかで終わる (設定がなければ約 2 秒かかり、待ちを越える)。経過時間を表示する | ループバックで SYN を送り直さない設定。拒まれた接続の行。IPv6 の any への bind | ioctl が失敗する: その場で `MaxSynRetransmissions = 1` にする。runner に IPv6 のループバックがなく `::1` だけが落ちる: その場で `::1` の回を外し、IPv6 は確かめていないと記録する (backend のテスト8も同じに外す) |
| P12 | `a_connect_ex_connects_after_a_bind_to_any` | P11 と同じ手順で std のリスナーへ → packet `Ok`。`SO_UPDATE_CONNECT_CONTEXT(null, 0)` → 0。`getpeername` がリスナーを返す。`shutdown(SD_SEND)` → サーバが EOF を読む | 接続の道 | 止める |
| P13 | `large_nonblocking_sends_block_within_eight_calls` | 読まない相手の対で、8 MiB のブロックしない `send` を繰り返すと、8 回以内に WOULDBLOCK になる (1回ごとのバイト数を表示) | T-4 の `WRITES_UNTIL_PENDING = 8` | その場で: T-4 の値を「見えた回数 + 2」にし、この試しの上限も合わせる |
| P14 | `a_connect_to_a_full_accept_queue_is_refused_or_stays_pending` | std のリスナーの列を満たす (`connect_timeout(50 ms)` が初めて失敗するまで、1024 まで)。そこへの `ConnectEx` は、すぐ拒まれるか、100 ms 以内に終わるか、保留のまま (そのときは取り消して packet を読む)。操作が終わることだけを確かめ、どれだったかを表示する | T-4 の行20 | なし (S2 では何も決めない) |
| P15 | `an_op_outlives_the_thread_that_issued_it` | 終わったスレッドが出した長さ 0 の読みは保留のまま残る。スレッドを join した後に相手が書くと、packet は 995 でなく `Ok(0)` | 仮定 A15 | 止める |

#### D-11: IOCP の backend の形

- データ:
  ```rust
  pub(crate) struct Sock { s: OwnedSocket, v6: bool }
  pub(crate) struct Iocp {
      waker: Arc<PortWaker>, epoch: Instant,          // 今のまま
      ext: Option<Ext>,                               // AcceptEx と ConnectEx の関数のポインタ。最初のソケットで読む
      boxes: HashMap<usize, Lent>,                    // OS が持つ箱すべて。番地で引く
      waiters: HashMap<OpKey, *mut Over>,             // 保留の鍵からその箱へ
  }
  struct Lent { key: OpKey, on: SOCKET, cancelled: bool }
  #[repr(C)] struct Over {
      ov: OVERLAPPED,                  // 先頭に置く。packet の番地が箱の番地になる
      on: SOCKET,                      // 呼び出しを出したソケット (AcceptEx ではリスナー)
      flags: u32,                      // WSARecv の lpFlags
      addrs: [u8; 2 * ADDR_LEN],       // AcceptEx の出力
      peer: SOCKADDR_STORAGE,          // ConnectEx の行き先
      what: What,                      // OS は触れない
  }
  enum What { Zero, Send, Accept { sock: Sock }, Connect }
  const WAKE: usize = 0; const SOCKET_KEY: usize = 1;
  const BACKLOG: i32 = 1024; const ADDR_LEN: usize = size_of::<SOCKADDR_STORAGE>() + 16; // 144
  const NO_SYN_RETRANSMISSIONS: u8 = 254;   // windows-sys は定数を u16 の 65534 で持つが、フィールドは u8 ((UCHAR)-2)
  const BOX_ABORT: &str = "eml_io: an OVERLAPPED box of a dropped I/O driver is still held by the kernel; aborting";
  ```
  roadmap は「記録ごとの `OVERLAPPED` の箱」と書くが、箱は記録の行でなく、この2つの表に置く。packet の番地を、中を読む前に `boxes` で確かめるため (番地で引く表) と、`cancel` が鍵しか受け取らないため (鍵で引く表) である。G-3 の「生きた箱の数と待ち手の数が等しい」は `debug_assert_eq!(boxes.len(), waiters.len())` で確かめる
- 番地の規則 (必須): 箱の中への番地はどれも `Box::into_raw` が返した `*mut Over` から `&raw mut (*over).field` で作る。`into_raw` から `from_raw` までの間に `Over` やそのフィールドへの `&` と `&mut` を作らない。OS が書く buffer は `What` の中に置かず、`Over` のフィールドにする。`WSABUF` は呼び出しの場の局所変数にする
- `WSAStartup`: 関数 `startup()` が `static STARTED: OnceLock<i32>` に `WSAStartup(0x202, …)` の戻りを持ち、0 でなければ `Err(from_raw_os_error(code))` を返す。`Iocp::new()` が最初に呼ぶ。`WSACleanup` は呼ばない (std と同じく、プロセスと同じだけ生かす)
- 拡張関数: `AcceptEx` と `ConnectEx` を、backend が最初に作るソケットで、結び付ける前に `WSAIoctl(SIO_GET_EXTENSION_FUNCTION_POINTER, WSAID_*)` で1回だけ読む。得られなければ `Err(Other "Winsock lacks AcceptEx or ConnectEx")`。`GetAcceptExSockaddrs` は使わず、相手は `SO_UPDATE_ACCEPT_CONTEXT` の後の `getpeername` で読む
- 補助の関数: `stream_socket(v6)` (`WSASocketW(AF_INET | AF_INET6, SOCK_STREAM, IPPROTO_TCP, null, 0, WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT)` の後、`ext` がなければ読む)、`nonblocking` (`ioctlsocket(FIONBIO, 1)`)、`associate` (`CreateIoCompletionPort(s, port, SOCKET_KEY, 0)`)、`check`、`issue`
  - `issue(key, on, what, error, call) -> Start` が `Box::into_raw` の唯一の場所で、同期の失敗の `Box::from_raw` を持つ。出せたか同期に成功した → 2つの表に入れて `Start::Pending(Hold::Memory)`。ほかの番号 → `drop(Box::from_raw(over))` (中の受けのソケットも閉じる) で `Start::Done(Err(error(code)))`
- 関数:
  - `listen(addr)`: `stream_socket` → `bind` → `listen(1024)` → `getsockname` でポート → FIONBIO → 結び付け。`SO_REUSEADDR` も `SO_EXCLUSIVEADDRUSE` も立てない (前者は使用中のポートを別のソケットに取らせ、後者は古い接続が TIME_WAIT にある間、立ち直したサーバを `AddrInUse` にする)。そのため、同じ利用者の別のソケットが同じポートのより狭い番地に bind できる。`IPV6_V6ONLY` は OS の既定のまま
  - `socket(peer)`: `stream_socket` → `0.0.0.0:0` か `[::]:0` に bind。`peer.ip().is_loopback()` なら `WSAIoctl(SIO_TCP_INITIAL_RTO, {Rtt: 0, MaxSynRetransmissions: 254})` を出し、結果は捨てる (速さのためだけで、P11 と backend のテスト8が CI で確かめる)。続けて FIONBIO → 結び付け
  - `shutdown_write`: `shutdown(SD_SEND)`。`set_nodelay`: `setsockopt(IPPROTO_TCP, TCP_NODELAY, &i32, 4)`
  - `close(sock)`: `debug_assert!(boxes.values().all(|l| l.on != sock.raw()))` の後に落とす
  - `start`:
    - `Peek`: `recv(s, &byte, 1, MSG_PEEK)`。0 以上 → `Done(Ok(Size(0)))`。WOULDBLOCK → 長さ 0 の読み (`issue(key, s, Zero, stream_error, WSARecv(s, &WSABUF{0, null}, 1, null, &raw mut (*over).flags, &raw mut (*over).ov, None))`)。誤り → `Done(Err(stream_error))`
    - `Recv`: `recv(s, ptr, min(cap, i32::MAX), 0)`。`n` → `Size(n)`。WOULDBLOCK → 長さ 0 の読み。誤り → `stream_error`
    - `Send`: `send(s, ptr, min(cap, i32::MAX), 0)`。`n` → `Size(n)`。WOULDBLOCK → `issue(key, s, Send, stream_error, WSASend(s, &WSABUF{min(cap, u32::MAX), ptr}, 1, null, 0, ov, None))`。貸された `Write` の記憶をそのまま OS に渡し、`Hold::Memory` が覆う
    - `Accept`: `accept(l, &storage, &len)`。成功 → FIONBIO → `SetHandleInformation(s, HANDLE_FLAG_INHERIT, 0)` → 結び付け (どれかの失敗はそのまま返し、受けたソケットは閉じる)。相手が IP でなければ `Other`。WOULDBLOCK → `stream_socket(l.v6)` (失敗 → `Done(Err)`) の後、`issue(key, l, Accept{sock}, accept_error, AcceptEx(l, s, &raw mut (*over).addrs, 0, 144, 144, &local_bytes, ov))`。受けのソケットは `AcceptEx` の前に結び付けない (packet はリスナーを通って来る)。ほかの誤り → `accept_error`
    - `Connect`: `&raw mut (*over).peer` に番地を書き、`issue(key, s, Connect, os_error, ConnectEx(s, peer_ptr, len, null, 0, null, ov))`
  - `cancel(key, _step)`: `waiters[key]` がない → `debug_assert!(false, "a cancel for a key with no box")` の上で `Ok(Cancel::Done)`。`CancelIoEx(lent.on, over)` が成功するか `ERROR_NOT_FOUND` → 印を立てて `Ok(Cancel::Pending)`。ほかの失敗 → 印を立てずに `Err`
  - `wait`: `GetQueuedCompletionStatusEx(port, [64], …, FALSE)`。`Err` を返すのはこの呼び出しが `WAIT_TIMEOUT` 以外で失敗したときだけで、何も積む前である。項目ごとに、`WAKE` → `posted` を下ろす。`SOCKET_KEY` で番地が null でない → `boxes` になければ `debug_assert!(false)` で捨てる。あれば2つの表から外し、`Box::from_raw` し、`Event{key: lent.key, arrival(box, lent.cancelled)}` を積む。ほか → `debug_assert!(false)` で捨てる
  - `arrival(over, cancelled)`: 箱を取り戻してから `WSAGetOverlappedResult(over.on, &over.ov, …, FALSE, …)` で状態を読む

    | 箱 | 状態 | Arrival |
    |---|---|---|
    | どれも | `Err(995)`、印あり | `Cancelled` |
    | どれも | `Err(995)`、印なし | `Done(Err(Other "the OS aborted the operation"))` |
    | Zero | `Ok` | `Retry` |
    | Zero | `Err(c)` | `Done(Err(stream_error(c)))` |
    | Send | `Ok(n)` / `Err(c)` | `Done(Ok(Size(n)))` / `stream_error` |
    | Accept | `Ok` | `SO_UPDATE_ACCEPT_CONTEXT(&on)` → FIONBIO → 結び付け → `getpeername` → `Done(Ok(Accepted))`。前の3つの失敗はそのまま返し、`getpeername` の失敗は `gone` を通す |
    | Accept | `Err(c)` | `accept_error(c)` |
    | Connect | `Ok` | `SO_UPDATE_CONNECT_CONTEXT(null, 0)` → `Done(Ok(Connected))` か `Err` |
    | Connect | `Err(c)` | `os_error(c)` |
  - `Drop`: `boxes` が空でなければ `eprintln!(BOX_ABORT)` の後に `process::abort()`
- `Backend::SOCKETS` は消す。定数、核の `!B::SOCKETS` の3つの塊、`fn unsupported`、`Fake<const SOCKETS>`、テスト `sockets_off_rejects_before_looking_up_the_handle` がなくなる。どの backend も真にするためである
- 試作の印 (G-10) は T-3 で変える。`backend.rs` の `cfg_attr(all(windows, not(test)), expect(dead_code))` の5つ (`Step`、`Outcome`、`Start`、`Cancel::Done`、`Arrival::Retry`) をコメントごと消す。`cfg_attr(not(test), …)` の4つ (`Hold::Memory`、`Cancel::Pending`、`Arrival::Done`、`Arrival::Cancelled`) を `cfg_attr(not(any(windows, test)), …)` にする。試作でこの印のまま `.#cross` の clippy が通ることを確かめてある
- 退けた案: 試しを `tests/winsock.rs` に置く (backend のテストの場所でなく、G-3 の単体テストにならない)。鍵で引く表1つ (確かめる前に迷った packet の箱を放しうる)。番地の表1つと取り消しでの線形の探索 (`quiesce` で O(n²))。unix と同じ世代つきの行の表 (起こらないことを防ぐだけ)。`SO_EXCLUSIVEADDRUSE`。`AcceptEx` を mswsock.dll から link する。`OVERLAPPED.Internal` を NTSTATUS として読む (roadmap は `WSAGetOverlappedResult` を求める)。`WSACleanup`。書きのデータを箱に写す。T-3 を分ける (`SOCKETS` の削除、backend、削除するテストは同時でないと緑にならない)

#### D-12: Windows の誤りの種類

- 規範の置き場所は vm.md の「時計と driver」の `Listen`、`Accept`、`Connect` の箇条である (T-3 が書く)
- そろえ方: `os_error(code)` は 1225 → `ConnectionRefused`、1236 → `ConnectionAborted`、64 → `ConnectionReset` にし、どれも `io::Error::new(kind, from_raw_os_error(code))` で包む。ほかの番号は std の写し方に任せる。`stream_error` は加えて `ConnectionAborted` を `ConnectionReset` にする。`accept_error` と `gone` は `ConnectionReset`、`ConnectionAborted`、`NotConnected` を `ConnectionAborted` にし (核が受けを出し直す)、ほかはそのまま返す。995 は std に渡さない (std は `TimedOut` にする)

| 場合 | Windows の番号 (見込み) | 種類 | unix の種類 |
|---|---|---|---|
| 拒まれた接続 | 10061 / 1225 | ConnectionRefused | 同じ |
| 接続の時間切れ | 10060 (か 121) | TimedOut | 同じ |
| ネットワークかホストに届かない | 10051 / 10065 / 1231 / 1232 | std の種類 | 同じ |
| RST の後の読みと覗き | 10054 / 64 / 10053 | ConnectionReset | 同じ |
| 閉じた相手への書き、RST で終わる保留の書き | 10054 / 10053 / 64 | ConnectionReset | BrokenPipe か ConnectionReset |
| 自分の `shutdown_write` の後の書き | 10058 | BrokenPipe | 同じ |
| 受ける前のリセットと中断 | 10054 / 10053 / 64、`getpeername` の 10057 | ConnectionAborted (出し直す) | ECONNABORTED (出し直す) |
| ほかの理由で受けの準備が失敗する | どれも | そのまま返す | Apple の `prepare` は出し直す。意図して違える |
| ソケットが尽きた | 10024 / 10055 | std の種類 (返す) | EMFILE (返す) |
| 同じ番地の2つ目のリスナー | 10048 | AddrInUse | 同じ |
| ホストにない番地 | 10049 | AddrNotAvailable | 同じ |
| 頼んだ取り消しの中断 | 995、印あり | `Cancelled` → 核の `Other "cancelled"` | 同じ |
| 頼んでいない中断 | 995、印なし | Other | なし |

- 種類でない違い: Windows では `::` のリスナーが IPv4 を受けない。同じ利用者の別のソケットが同じポートのより狭い番地に bind できる。1024 未満のポートに権限が要らない。満ちた受けの列への接続の振る舞いは P14 が報告するまで分からない

#### D-13: テストの移行の形

- `real.rs` の誤りの種類は、ファイルの先頭の2つの定数で書く。cfg で分けない
  ```rust
  const RESET_READ: &[ErrorKind] = &[ErrorKind::ConnectionReset];
  const PEER_GONE_WRITE: &[ErrorKind] = &[ErrorKind::BrokenPipe, ErrorKind::ConnectionReset];
  ```
  D-12 で Windows の組は `PEER_GONE_WRITE` の部分集合なので、1つの定数で足りる。assert は `CONST.contains(&e.kind())` で書く。`AddrInUse` と `ConnectionRefused` は定数にしない
- `write_until_pending` の上限は、新しい定数 `WRITES_UNTIL_PENDING: usize = 8` にする (今の 4096 回では最大 32 GiB を出しうる)。Linux では呼ぶ4か所とも1回目で保留になることを確かめてある
- `reset_on_close` は unix と Windows の腕を持つ。Windows は windows-sys の `setsockopt(SOL_SOCKET, SO_LINGER, LINGER{l_onoff: 1, l_linger: 0})` を `as_raw_socket() as usize` に当て、失敗は `io::Error::from_raw_os_error(WSAGetLastError())` にする。`unsafe` ごとに `// SAFETY:` を書く。依存は増えない
- `closed_port()` は今の bind して落とす形のまま残す。unix で今も使っている形で、競合の見込みは低い
- `tls.rs` は `"localhost"` でなく `"127.0.0.1"` に接続する。fixtures の証明書は SAN に `IP:127.0.0.1` を持ち、`eml_tls` は IP の名前を受け付ける。名前の解決と `::1` の順に頼らなくなる
- 実時計の待ち (`wait_for` の 1 秒、`Gate::wait_started` の 1 秒、`api.rs` の 3 秒) は、testing.md の「性能のテスト」が認める見張りで、上限の時間を足さない。Windows で見張りが切れたら、testing.md の「新しい対象で落ちたテストの仕分け」に従い、runner で測ってから 10 倍の規則で延ばす。再試行はしない

#### D-14: コンソールの事象から `Terminate` への写し

- 対応 (`crates/eml_cli/src/interrupt/windows.rs`):

| 事象 | `Mode::Run` (`eml run`) | `Mode::Repl` |
|---|---|---|
| `CTRL_C_EVENT` | 今のまま。`respond(INTERRUPT)`。Handled → TRUE。Exit(130) → `TerminateProcess(130)` | 今のまま |
| `CTRL_CLOSE_EVENT`、`CTRL_SHUTDOWN_EVENT` | `respond(TERMINATE)` の後、handler は返らない (Hold)。続けて来ても Hold するだけで、終わらせない | FALSE (OS の既定) |
| `CTRL_LOGOFF_EVENT` | 写さない。FALSE | FALSE |
| `CTRL_BREAK_EVENT` | FALSE (今のまま) | FALSE (今のまま) |
| `INSTALLED` を置く前の事象 | FALSE | FALSE |

- 形:
  ```rust
  #[derive(Debug, PartialEq)]
  enum Action { Pass, Handled, Hold, Exit(u8) }

  unsafe extern "system" fn on_console_event(event: u32) -> BOOL {
      act(console_event(INSTALLED.get(), event))
  }
  fn act(action: Action) -> BOOL {
      match action {
          Action::Pass => FALSE,
          Action::Handled => TRUE,
          Action::Hold => loop { std::thread::park(); },
          Action::Exit(code) => { terminate(code.into()); TRUE }
      }
  }
  fn console_event(installed: Option<&Installed>, event: u32) -> Action {
      let Some(installed) = installed else { return Action::Pass };
      match event {
          CTRL_C_EVENT => match installed.respond(Control::INTERRUPT) {
              Response::Handled => Action::Handled,
              Response::Exit(code) => Action::Exit(code),
          },
          CTRL_CLOSE_EVENT | CTRL_SHUTDOWN_EVENT if matches!(installed.mode, Mode::Run) => {
              let _ = installed.respond(Control::TERMINATE);
              Action::Hold
          }
          _ => Action::Pass,
      }
  }
  ```
  `respond` は `Exit` を返すときビットに触れないので、2回目の `Exit` を捨ててよい。`mod.rs` の処理は変えず、`Mode` と `TERMINATE_EXIT_CODE` のコメントだけを直す
- 理由:
  - 返らないのは、閉じる、ログオフ、シャットダウンでは handler が返ったところでプロセスが終わるからである。Go と tokio も止めたまま待つ。自前の期限で返しても、早く死ぬだけである
  - 2回目で終わらせないのは、unix の2回目の SIGTERM が人による強い要求なのに対し、Windows のこれらの事象は OS が送り (閉じた後のシャットダウンなど)、安全点に来ない処理からの出口は OS の待ち時間だからである。Ctrl-C は今までどおり2回目で終わる
  - 止めたスレッドはロックを持たない。`doorbell.ring()` は止める前に Windows の doorbell の mutex を放す。プロセスがどの道で終わっても (main が返る、eml の `exit`、2回目の Ctrl-C、panic、OS の待ち時間切れ)、止めたスレッドごと終わる
  - ログオフを写さないのは、受け手が主にサービスで、サービスにはどの利用者のログオフでも届くからである。サービスでない、user32 を読み込まないコンソールのプロセスだけが得をするが、今の `eml` はそれに当たらない
  - シャットダウンは、今の `eml` に届かない見込みでも写す。roadmap の前提で、腕1つで済み、REPL から user32 の依存が外れれば届く
- 結果: 待つタスクがなければ、次の安全点で止まり、`Machine::run` が flush して、main は 143 を返す。待つタスクがあれば、プログラム自身の終了コードで終わる。ただしコンソールを閉じた後は、コンソールへの書き込みと読みが先に失敗しうる。書き込みの誤りなら終了コードは 1 になる。出力をファイルか pipe に向ければこの失敗は起きない
- 退けた案: `respond` の後に TRUE を返す (すぐ死ぬ)。自前の期限か、main から鳴らす condvar で返す。2回目の閉じるで `TerminateProcess(143)` にする (unix の写し)。ログオフを写す (Go の選択)。REPL で閉じるを扱う (止める語を読む者がいない)

#### D-15: 文書を誰がいつ直すか

- T-3 で偽になる文は T-3 で消し、新しい文は持ち主のタスクが書く
- 持ち主: T-3 は `docs/spec/*` (vm.md と stdlib.md のソケットの部分)、architecture.md の `sys/iocp.rs` の箇条と windows-sys の行、async.md の 244 行、322 行、324 行、CLAUDE.md。T-4 は status.md (ソケットの文)、testing.md、async.md の「テストと決定性」、architecture.md のテストの箇条、`tls.rs` のコメント。T-5 は vm.md の「上限と割り込み」、stdlib.md の「`Std.Signal`」、repl.md、async.md のシグナルの箇条と `drain_within` の段落、status.md (`Terminate` の文)、testing.md のシグナルの段落
- status.md の 150 行は T-3 と T-5 が同じ箇条を直す。T-3 がソケットの文を消し、T-5 が `Terminate` の文を消して、残った「[ロードマップ](../future/roadmap.md) の `net-windows`」の引用も消す
- T-2 の vm.md の文は、完了型の backend 一般の約束だけを書き、IOCP に固有の文 (長さ 0 の `WSARecv`、箱の所有) は T-3 で書く。T-2 の時点で IOCP の backend はまだソケットを持たず、文書は今の状態を書くためである

#### D-20: Windows の CI で確かめる手順

- このコンテナには `gh` がないので、GitHub MCP の `actions_run_trigger` (`workflow_dispatch`、`ref` は段のブランチ)、`actions_get`、`get_job_logs` を使う
- 各タスクのコミットを段のブランチに push し、`workflow_dispatch` で CI を起こす。Windows の job が赤なら、そのタスクのコミットを直して (`git commit --amend`) `git push --force-with-lease` で出し直す。緑になるまで次のタスクに進まない (Q-1)
- 試しの表示の値 (P8、P11、P13、P14 の `eprintln!`) は、CI が失敗したテストの出力しか表示しないので、使い捨てのコミットで Windows のテストの段を `cargo nextest run -p eml_io --success-output immediate -E 'test(sys::iocp::tests::)'` に変えて1回流し、表示を写してから、そのコミットを外す。値は T-1 のコミットのメッセージに書く

## 6. 実装の約束

- 試しの「止める」の結果が出たら段を止め、落ちた assert とログをユーザーに見せる。T-2 に進まない (D-10)
- 試しの「その場で」の結果は、試しを見えた振る舞いを確かめる形に直し、どの代わりを当てたかを T-1 のメッセージに書く。T-3 はそれを実装する (D-10)
- 試しが環境やテストの誤りで落ちたとき (`::1` がない、補助の関数の誤りなど) は、ブランチの上で直してメッセージに書く (D-10)
- `into_raw` から `from_raw` までの間に `Over` への参照を作らない (D-11)
- 箱を放すのは `wait` の packet の取り出しと、`issue` の同期の失敗の2か所だけ (D-4)
- コードとコメントに計画の ID (D-x、T-x) と `docs/plans/` を書かない。規則は spec か実装の文書の節を引く
- コメントと文書は日本語で、natural-japanese のスキルに従う。コメントは理由を書く
- 各テストは単独で 1 秒以内に終わり、結果が機械の速さで変わらない。経過時間を assert しない (D-10、D-13)
- テストの変更は、7節のタスクごとに書いた種類 (成否の変更、期待値の変更、機械的な追随) と理由をコミットのメッセージに書く

## 7. 実装のタスク

| ID | タスク | 基づく決定 | 依存 | 並行 |
|---|---|---|---|---|
| T-1 | Windows の試し (テストだけ) | D-10、D-20 | - | 不可 |
| T-2 | 境界と核 | D-1、D-2、D-3、D-5、D-15 | T-1 | 不可 |
| T-3 | IOCP のソケットの backend | D-4、D-11、D-12、D-15、D-20 | T-2 | 不可 |
| T-4 | 本物のネットのテストを Windows で流す | D-13、D-15、D-20 | T-3 | 不可 |
| T-5 | コンソールの事象から `Terminate` | D-14、D-15、D-20 | T-4 (status.md の受け渡しのため) | 不可 |
| T-6 | 段を閉じる (`Close net-windows`) | roadmap の「進め方」 | T-5 | 不可 |

### T-1: Windows の試し

- 目的: 文書にない OS の振る舞いを、backend を書く前に Windows の CI で確かめる
- 対象: `crates/eml_io/Cargo.toml`、`crates/eml_io/src/sys/iocp.rs`、`docs/implementation/testing.md`
- 作業:
  - Cargo.toml に次を足す。lib は変わらない
    ```toml
    [target.'cfg(windows)'.dev-dependencies]
    windows-sys = { workspace = true, features = ["Win32_Networking_WinSock"] }
    ```
  - `sys/iocp.rs` の末尾に `#[cfg(test)] mod tests` を足し、`Rig`、補助の関数、P1 から P15 を書く (D-10。付録の `probes-prototype.diff`)
  - testing.md の「テストの置き場所」の4行目に足す: IOCP の backend のテストは `src/sys/iocp.rs` の `mod tests` に本物のソケットで置き、backend が頼る文書にない OS の振る舞い (長さ 0 の `WSARecv`、`OVERLAPPED` なしの呼び出しが packet を積まないこと、`accept` と `AcceptEx` の併用、ループバックで SYN を送り直さないこと) を確かめるテストを含む
  - D-20 の手順で Windows の CI を緑にし、表示の値をメッセージに写す
- 実装者に任せること: `unsafe` ごとの SAFETY のコメント、理由のコメント、補助の関数の名前。assert は D-10 の「落ちたとき」の列の代わりでだけ変える
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_io` → すべて通る
  - `cargo clippy --all-targets -- -D warnings` → 警告なし
  - 3節の `.#cross` の clippy → 警告なし
  - `cargo test -p eml_io --lib trust` → 通る
  - `git show --stat HEAD` → 上の3ファイルだけ
  - Windows の CI のログを `log.txt` に保存し、次が何も表示しない
    ```sh
    for t in a_zero_read_waits_for_data_and_leaves_it a_zero_read_completes_on_a_closed_peer a_zero_read_ends_on_a_reset a_zero_read_on_buffered_data_queues_a_packet_that_a_cancel_does_not_find a_cancelled_zero_read_ends_aborted calls_without_an_overlapped_queue_no_packet an_overlapped_send_that_completes_at_once_still_queues_a_packet an_overlapped_send_on_a_full_socket_pends_and_cancels accept_and_accept_ex_share_a_listener a_cancelled_accept_ex_ends_aborted a_refused_loopback_connect_ex_fails_fast a_connect_ex_connects_after_a_bind_to_any large_nonblocking_sends_block_within_eight_calls a_connect_to_a_full_accept_queue_is_refused_or_stays_pending an_op_outlives_the_thread_that_issued_it; do grep -q "PASS .*sys::iocp::tests::$t$" log.txt || echo "missing $t"; done
    ```
    `grep -c 'FAIL\|SIGABRT' log.txt` → 0
  - コミットのメッセージに P8、P11、P13、P14 の表示の値と、当てた代わりがある
- 落とし穴: `only_marks` が強いのは、戻る前に終わった呼び出しについてだけである。P11 に経過時間の assert を入れない。拒まれるポートを bind して落とす形で作らない (並行するテストに取られる)

### T-2: 境界と核

- 目的: 完了型の値を確定させ、落とすときの規則を D-3 にそろえ、trust.rs で指針を固定する
- 対象: `crates/eml_io/src/driver/backend.rs`、`driver/mod.rs`、`driver/tests/{fake.rs,mod.rs,abort.rs}`、`trust.rs`、`crates/eml_rt/src/driver.rs` (doc)、`docs/spec/vm.md`、`docs/implementation/architecture.md`、`docs/implementation/testing.md`
- 作業:
  - `backend.rs`: 名前を変え、doc コメントを D-1 の形にし、「暫定」の文 (`:4-5`、`:48-49`、`:64-65`、`:75`、`:81`、`:84`、`:94-96`) を消し、trait の doc に `wait` の規則を足す。`cfg_attr` の印は1つも変えない (G-10)。`sys/iocp.rs` は触らない
  - `driver/mod.rs`:
    - `Drop` の取り消しの対象を `!slot.cancelled` から `slot.cancel_sent.is_none()` にし、待ちの失敗ですぐ abort する塊を消す (付録 `core-prototype.diff` の `mod.rs` の差分)
    - `enum By` と `Broken.by` を消す (`Broken { kind, msg }`)
    - `arrive` の合わさった腕を分ける: `Arrival::Retry => { drop_kind; cancelled }` と `Arrival::Cancelled => { debug_assert!(slot.cancelled, "the backend reported a cancel the core did not ask for"); drop_kind; cancelled }`。欄は assert の前に外す
    - `Hold::Buffer` → `Hold::Memory`。`HELD_ABORT` を `"eml_io: the kernel may still write memory of a dropped I/O driver; aborting"` に、`HELD_LIMIT` のコメントの「buffer」を「記憶」にする
  - 偽の backend に `cancelled_keys: Arc<Mutex<Vec<OpKey>>>` を足し、`cancel` で記録する (落とした後も読める)
  - 核のテスト。既存の4件と abort の2件は名前を変えて残す。`a_broken_driver_with_a_held_buffer_aborts_on_drop` は `cancels.push_back(Ok(Cancel::Pending))` と2つ目の `wait_error` を足し、待ちが失敗し続けるようにする (期待値の変更: 待ちで壊れた driver も取り消しを出すようになったため)。新しいテスト:
    1. `a_retry_for_a_cancelled_held_op_ends_cancelled`: `Memory` の読み、取り消し → `Pending`、`Retry` → `cancelled` の完了が1つ、`Start` は1回
    2. `a_retry_for_a_held_op_restarts_the_step`: `[Pending(Memory), Fill("ab")]`、`Retry` → `Size(2)`、`Start` は2回
    3. `a_full_write_completed_after_a_cancel_succeeds`: 取り消し `Pending`、`Done(Ok(Size(cap)))` → `Ok`、`cancelled == false`
    4. `a_short_write_completed_after_a_cancel_ends_cancelled`: `Done(Ok(Size(2)))` → `cancelled`、2回目の `Start` なし
    5. `closing_a_handle_with_a_held_op_closes_after_the_event`: `close` は待ち、event の後の最後の呼び出しが `Close(0)`
    6. `drop_does_not_cancel_a_cancelled_held_op_twice`: 落とす前に `Cancel` が1回、`Cancelled` を積む、落とすと戻る
    7. `drop_after_a_broken_quiesce_cancels_every_slot_once`: `Memory` の読み2つ、`cancels = [Err]`、`quiesce` (壊れ、`pending() == 2`)、続けて `cancels = [Pending, Pending]` と `Cancelled` 2つ。落とすと戻り、`cancelled_keys.len() == 3` で 1 と 2 を含み、閉じたのは {0, 1}
    8. `a_driver_broken_by_a_failed_wait_still_reads_the_ends_on_drop`: `wait_error` 1回、`Memory` の読み、取り消し `Pending` と `Cancelled` → 落とすと戻る
    9. `an_event_for_an_unknown_key_closes_only_an_accepted_sock`: 古い `Done(Ok(Accepted{9}))` → `Close(9)`、完了なし
    10. `a_cancelled_arrival_without_a_cancel_is_a_contract_violation`: `#[cfg(debug_assertions)] #[should_panic]`。取り消していない欄に `Cancelled`
    11. `abort::a_refused_cancel_with_no_end_aborts_on_drop`: `cancels = [Err]` の後 `[Pending]`、event なし → 子が `HELD_ABORT` で abort
  - trust.rs: `const COMPLETION_BACKENDS: &[&str] = &["sys/iocp.rs"];` を足し、次の規則を置く。走査は既存の `prepare()` (コメントと文字列を空白にする) の上で行い、綴りを変えれば抜けられることは承知の上とする
    - T2 `completion_values_stay_in_the_core_and_completion_backends`: 4つの値は `driver/backend.rs`、`driver/mod.rs`、`driver/tests/*`、`COMPLETION_BACKENDS` にだけ現れる。`COMPLETION_BACKENDS` のファイルはどれも `UNSAFE_FILES` にある。`driver/mod.rs` と `driver/tests/*` で4回以上見つかる。既存の `readiness_backends_report_only_retries` はこれに合わせて消す
    - T3 `the_core_reads_hold_only_to_drop`: `driver/mod.rs` の `Hold::Memory` はどれも `fn drop` の中、`Hold::None` はどれも `fn submit` の中にある (`enclosing_fn` を使う)。3回以上見つかる
    - T4 `raw_boxes_stay_in_the_completion_backends`: 識別子 `into_raw`、`from_raw`、`OVERLAPPED` は `COMPLETION_BACKENDS` にだけ現れる (`has_unsafe` と同じく識別子で照合し、`from_raw_fd` などには当てない)
    - T5 `abort_is_named_only_where_memory_is_at_stake`: 識別子 `abort` は `driver/mod.rs`、`driver/tests/mod.rs` (`mod abort;` の行)、`driver/tests/abort.rs`、`COMPLETION_BACKENDS` にだけ現れる。`driver/mod.rs` で1回以上見つかる
  - 文書 (日本語は natural-japanese で整える):
    - vm.md の「時計と driver」の取り消しの箇条 (`:463`): 「OS に取り消しを出して終わりを待つ backend では、OS が取り消しを断ると driver は壊れた状態になる。OS が『取り消す操作がない』と答えたときは、終わりがすでに列にあるので待つ」
    - 同じ節の本物の driver の箇条 (`:469`) に足す: 「完了型の backend は、同期に失敗した呼び出しのほかは、成功を含めて結果を OS の知らせで読む。OS に出た歩の鍵には event がちょうど1つ届き、核は event を読むまで欄を放さない。核は保留の歩があるソケットを閉じない」
    - `Drop` の箇条 (`:473`) の「OS の待ちが失敗して壊れた driver は終わりを読む道を持たないので、そうした操作が残っていれば待たずにすぐ止める。どちらも、貸した記憶を放せないまま戻らないためである。今の backend はどれもそうした操作を作らないので、落とすときは待たない」を次の内容にする: 待つのは、OS がこちらの記憶 (貸した buffer か backend の記録) に完了まで書きうる操作である。壊れた driver も同じで、まだ OS に出していない取り消しを出し、1 秒まで終わりを読み、待ちが失敗するか 1 秒たてば止める。epoll と kqueue の backend はそうした操作を作らない
    - eml_rt の `IoDriver` の doc (`crates/eml_rt/src/driver.rs:94-96`) の「カーネルが buffer を持ちうる操作だけ終わりを待ち、上限までに終わらなければプロセスを止める。OS の待ちが失敗して壊れた driver は終わりを読めないので、そうした操作が残っていれば待たずにすぐ止める」を「OS がこちらの記憶 (貸した buffer か driver の記録) に書きうる操作だけ終わりを待ち、上限までに終わらないか待ちが失敗すればプロセスを止める」にする。`crates/eml_rt/CLAUDE.md` の関門が求める確認があれば従う
    - architecture.md の `backend.rs` の箇条 (`:365`): 暫定の文を消し、「`Hold::Memory` は OS が完了まで書きうる記憶があることを表し、核は driver を落とすときだけ読む。readiness の backend は `Retry`、`Pending(Hold::None)`、`Cancel::Done` だけを使い、完了型の backend はすべての値を使う。`wait` は event を積んだ後に `Err` を返さない」を足す
    - testing.md の「`unsafe` の一覧」(`:200`) の `readiness_backends_report_only_retries` の行を、T2 から T5 の行に替える
- 実装者に任せること: テストの名前の細部、コメントの文
- 条件: いつも
- 完了の条件:
  - `cargo nextest run -p eml_io` → すべて通る (新しい核のテスト 11 件と trust の4規則を含む)
  - `grep -rn "暫定\|provisional\|Hold::Buffer\|By::Wait" crates/eml_io/src crates/eml_rt/src/driver.rs docs/spec docs/implementation` → 出力なし (roadmap は段を閉じるまで残す)
  - `grep -rnow "into_raw\|from_raw\|OVERLAPPED" crates/eml_io/src --include=*.rs | grep -v "sys/iocp.rs\|trust.rs"` → 出力なし
  - `git show HEAD -- crates/eml_io/src/driver/backend.rs | grep '^[-+].*cfg_attr'` → 出力なし
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 警告なし
  - 3節の `.#cross` の clippy → 警告なし
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib -- driver::tests::` → 通る。`-Zmiri-tree-borrows` を足してもう1回 → 通る
  - `grep -n '今の backend はどれもそうした操作を作らないので\|待たずにすぐ止める' docs/spec/vm.md` → 出力なし
  - D-20 の手順で Windows の CI → 緑
- 落とし穴: `cfg_attr` の印を変えると Windows の clippy が落ちる (T-3 まで `sys/iocp.rs` は値を作らない)。テスト10 は `#[cfg(debug_assertions)]` にしないと release で落ちる

### T-3: IOCP のソケットの backend

- 目的: Windows の本物の driver でソケットを使えるようにする
- 対象: `crates/eml_io/Cargo.toml`、`src/sys/iocp.rs`、`src/sys/mod.rs`、`src/sys/unix.rs`、`src/driver/backend.rs`、`src/driver/mod.rs`、`src/driver/tests/{fake.rs,mod.rs}`、`crates/eml_io/tests/real.rs`、`crates/eml_vm/tests/api.rs`、`crates/eml_cli/tests/tls.rs` (コメント)、`docs/spec/vm.md`、`docs/spec/stdlib.md`、`docs/implementation/{architecture.md,status.md,testing.md}`、`docs/future/async.md`、`CLAUDE.md`
- 作業:
  - Cargo.toml: windows-sys の feature を `["Win32_Foundation", "Win32_Networking_WinSock", "Win32_System_IO", "Win32_System_Threading"]` にし、T-1 の dev-dependencies の塊を消す
  - `sys/iocp.rs`: D-11 の backend を書く (付録 `backend-prototype.diff` を T-2 の後に当て直す)。モジュールの doc を「Windows の backend (IOCP)」にし、vm.md の「時計と driver」を引く。`WAKE` の「ソケットの packet を足すときは別の key を使う」のコメントを消す。試しを backend の補助の関数に切り替える (assert は変えない。機械的な追随)
  - backend のテストを同じモジュールに足す。どれも 1 秒の見張りの中で終わる
    1. `sockets_are_not_inherited`: リスナー、`socket()` のソケット、`accept()` で受けたソケット、`AcceptEx` のソケットのどれも `HANDLE_FLAG_INHERIT` を持たない
    2. `a_cancelled_pending_probe_ends_cancelled`: `Peek` → `Pending(Memory)`、`cancel` → `Pending`、event `Cancelled`
    3. `a_cancel_after_the_packet_is_queued_is_pending`: データを溜めてから長さ 0 の読み → `Pending`、`cancel` → `Pending`、event は `Retry` か `Cancelled`
    4. `a_cancel_without_a_box_is_a_contract_violation`: `#[cfg(debug_assertions)] #[should_panic]`
    5. `an_overlapped_send_through_the_backend_ends_with_its_size`: 空のストリームで `WSASend` → `Pending(Memory)`、event `Done(Ok(Size(4)))`、相手が `abcd` を読む (G-3 の backend の水準のテスト)
    6. `a_cancelled_pending_write_ends_once`: 8 MiB の `start(Send)` を `Pending` まで8回まで、`cancel` → `Pending`、event は `Cancelled` か `Done(Ok(Size(_)))`
    7. `a_cancelled_pending_accept_ends_cancelled`
    8. `a_refused_loopback_connect_ends_refused`: `127.0.0.1` と `::1` で、`socket` と `start(Connect)` から `refusing(ip)` へ。1 秒の見張りの中で `ConnectionRefused`
    9. `a_connect_ends_connected`: `Pending` の後に `Connected`。`shutdown_write` → 相手が EOF を読む
    10. `an_abort_nobody_asked_for_is_other`: 保留の長さ 0 の読みに `CancelIoEx` を直接出す → `Done(Err(Other))`
    11. `os_errors_take_the_kinds_of_the_table`: D-12 のそろえ方の行を、OS を呼ばずに確かめる
  - 核と周り: `backend.rs` の印を D-11 のとおりに変え、`const SOCKETS` と「ソケットを扱わない」のコメントを消す。`driver/mod.rs` の `fn unsupported` と `!B::SOCKETS` の3つの塊を消す。`sys/unix.rs` の `const SOCKETS` を消す。`sys/mod.rs` の doc を「Windows は IOCP で、`iocp.rs` が表と歩と待ちを持つ」にする。`Fake<const SOCKETS>` を `Fake` に、`calls<const S>` を `calls` にする (機械的な追随)。CLAUDE.md の Architecture の `IOCP without sockets` を `IOCP` にする
  - テストの成否の変更 (1件ずつ):
    1. `eml_io` の lib `driver::tests::sockets_off_rejects_before_looking_up_the_handle` を消し、代わりに `an_op_on_an_unknown_handle_reaches_no_backend` を足す。残す assert は、ハンドル 0 の `Readable` → InvalidInput、`shutdown_write(0)` と `set_nodelay(0, true)` → InvalidInput "no such handle"、`backend.calls` が空、`spawned == 0`
    2. `eml_io` の integration `real::listen_accept_and_connect_are_unsupported` を消す。Windows にソケットが入り、前提が偽になるため
    3. `eml_vm` の integration `api::a_real_listen_is_not_available` を doc コメントごと消す。理由は2と同じ
  - 文書 (T-3 が持ち主のもの。日本語は natural-japanese で整える):
    - vm.md の「時計と driver」の `Listen` の箇条 (`:468`): ソケットはブロックせず、子のプロセスに受け継がれない (unix は CLOEXEC、Windows は継承しない設定)。backlog は 1024。unix は `SO_REUSEADDR` を立てる。Windows は `SO_REUSEADDR` も `SO_EXCLUSIVEADDRUSE` も立てない (理由は D-11)。そのため同じ利用者の別のソケットが同じポートのより狭い番地に bind できる。`IPV6_V6ONLY` は OS の既定で、Windows では `::` のリスナーは IPv4 を受けない。受けの出し直しに「Windows では受ける前のリセットと、受けた直後に相手がいなくなった接続」を足し、受けの準備のほかの失敗は返すと書く。接続に「ループバックへの接続には SYN を送り直さない設定を出すので、拒まれた接続はすぐ誤りになる (Windows)」を足す。誤りの種類の文を足す (D-12 のそろえ方の3点: `ERROR_CONNECTION_REFUSED`、`ERROR_NETNAME_DELETED`、`ERROR_CONNECTION_ABORTED` を WSA の番号にそろえる。ストリームでは `WSAECONNABORTED` を `ConnectionReset` にする。頼んでいない中断は `Other`)。「Windows の本物の driver はソケットを扱わないので … `Unsupported` である」の2つの文を消す
    - 同じ節の本物の driver の箇条 (`:469`) の「Windows の backend はソケットを持たず、起こしと時間切れだけを待つ」を次の内容にする: IOCP の backend は、ソケットを作るか受けたときにポートへ結び付ける。読みは先にブロックしない `recv` (`Readable` は1バイトの `MSG_PEEK`) を試し、塞がれたら長さ 0 の `WSARecv` で待つ。その成功の知らせで同じ歩を出し直し、誤りの知らせはその誤りで終わる。読みの buffer は OS に渡さない。書きと受けは先に `send` と `accept` を試し、塞がれたときだけ `WSASend` と `AcceptEx` を出す。接続は any の番地に bind してから `ConnectEx` で出す。成功のときに知らせを省くモードは使わない。カーネルは取り消した後も知らせが届くまで backend の `OVERLAPPED` の箱に書くので、箱は知らせを読んだときにだけ放し、読まずに箱を放す道はプロセスを止める
    - stdlib.md の 1319 行の箇条を消す
    - architecture.md の `sys/iocp.rs` の箇条 (`:373`): Windows の backend。`Sock` はソケットとアドレスの族。行の表は持たず、OS が持つ箱を番地の表 (`boxes`。知らせの箱を取り戻す前に確かめる) と鍵の表 (`waiters`。`cancel` のため) に置く。Winsock はプロセスごとに1回始める。`AcceptEx` と `ConnectEx` は `WSAIoctl` で得る。操作ごとの `OVERLAPPED` の箱は backend が持ち、OS に渡してから知らせを読むまで所有を OS に預ける (`into_raw` と `from_raw` はこのファイルだけ)。箱を放すのは、知らせを読んだときと呼び出しが同期に失敗したときだけで、取り消しも `close` も箱に触れない。`AcceptEx` の受けのソケットは箱の中にあり、成功の知らせでだけ箱を出る。箱が残ったまま backend が落ちるときはプロセスを止める
    - architecture.md の 444 行の windows-sys の行に、Winsock の用途 (ソケット、`WSARecv` と `WSASend`、`WSAIoctl` で得る `AcceptEx` と `ConnectEx`、`CancelIoEx`、`WSAGetOverlappedResult`) を足す。backend.rs の箇条に `SOCKETS` が残っていれば消す
    - async.md の 244 行、322 行、324 行を消す
  - T-4 の持ち主の文書から、T-3 で偽になる所を消す (新しい文は T-4 が書く):
    - status.md の 150 行: 「Windows の backend はソケットを扱わないので、`Std.Net` の `listen`、`accept`、`connect` は種類 `NotAvailable` の IO の失敗になる。」
    - testing.md の 233 行: 「、Windows の `listen` が種類 `NotAvailable` になること」
    - testing.md の 478 行: 「Windows の本物の driver はソケットを扱わないためである。」
    - `crates/eml_cli/tests/tls.rs` の 1-2 行: 「Windows の本物の driver にはソケットがないので unix だけで流す」の節 (「本物の driver の上で `Std.Tls` が…」で始まる文の前半は残す)
    - architecture.md の 377 行: 「Windows のソケットの操作が `Unsupported` であることと、」
    - async.md の 389 行: 「、Windows では `listen` が `NotAvailable` になることを確かめる」 (文は「unix のものは `#[cfg(unix)]` にする」で終わる)
- 実装者に任せること: SAFETY と理由のコメント、名前、関数の分け方
- 条件: T-1 で「止める」が出ていないこと。「その場で」の代わりはここで実装する
- 完了の条件:
  - `cargo nextest run` → すべて通る
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 警告なし
  - `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'` → 通る
  - 3節の `.#cross` の clippy → 警告なし。同じコマンドの `--target` を `x86_64-unknown-linux-gnu` と `aarch64-unknown-linux-gnu` にしたもの → 警告なし
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib -- driver::tests::` → 通る
  - `grep -ow from_raw crates/eml_io/src/sys/iocp.rs | wc -l` → 2。`grep -ow into_raw crates/eml_io/src/sys/iocp.rs | wc -l` → 1。`grep -c 'Hold::None' crates/eml_io/src/sys/iocp.rs` → 0
  - `grep -rn 'SOCKETS\|fn unsupported' crates/eml_io/src` → 出力なし
  - `grep -rn 'ソケットを扱わ\|ソケットを持たず\|ソケットがない\|without sockets\|NotAvailable になる\|Unsupported であること' crates docs/spec docs/implementation docs/future/async.md CLAUDE.md` → 出力なし
  - `git show HEAD -- crates/eml_io/src/driver/backend.rs | grep '^[-+].*cfg_attr'` → D-11 の5つの削除と4つの変更だけ
  - Windows の CI が緑。T-1 の名前のループに backend のテスト 11 件の名前を足して → 出力なし
- 落とし穴: packet の番地は、中を読む前に `boxes` で確かめる。`Accept` の後処理の失敗を一律に出し直しにしない (本物の接続を落としながら空回りする)。backend の `unsafe` は Windows の Miri でも ASan でも確かめられない

### T-4: 本物のネットのテストを Windows で流す

- 目的: unix と同じ本物のネットのテストの組を Windows で流す
- 対象: `crates/eml_io/tests/real.rs`、`crates/eml_vm/tests/api.rs`、`crates/eml_cli/tests/{main.rs,tls.rs}`、`docs/implementation/{testing.md,architecture.md,status.md}`、`docs/future/async.md`
- 作業: 付録 `tests-prototype.diff` から、T-3 に入った2件の削除と Cargo.toml の行を除いて当てる。そのうえで次を守る
  - `real.rs` の cfg (行番号は基点のもの)。「なし」は cfg を外して全 OS で流す

    | # | テスト (行) | 今 | 後 | 種類 | 備考 |
    |---|---|---|---|---|---|
    | 1 | `a_wake_from_another_thread_leaves_a_wait_working` (128) | なし (中身が unix と windows に分かれる) | なし。中身を1つにする (`connection` と `Readable`。`Op::Job` の腕は消す) | 機械的な追随 | |
    | 2 | `a_readable_probe_stays_pending_without_data` (259) | unix | なし | 成否 | |
    | 3 | `a_probe_with_data_finishes_at_submit` (296) | unix | なし | 成否 | |
    | 4 | `a_read_after_a_probe_returns_the_bytes` (333) | unix | なし | 成否 | |
    | 5 | `a_write_is_read_by_the_peer` (371) | unix | なし | 成否 | |
    | 6 | `a_cancelled_probe_completes_with_an_error` (404) | unix | なし | 成否 | |
    | 7 | `a_probe_on_a_reset_stream_fails_at_submit` (419) | unix | なし | 成否と期待値 | 最初の覗きが保留なら、その完了自体がリセットの誤りでもよい。種類は `RESET_READ` |
    | 8 | `a_closed_peer_reads_eof` (458) | unix | なし | 成否 | |
    | 9 | `quiesce_drains_a_pending_probe_a_full_write_and_a_running_job` (486) | unix | なし | 成否 | |
    | 10 | `pool_threads_block_interrupt_and_terminate` (529) | unix | unix | なし | Windows はシグナルを塞がない (vm.md:379) |
    | 11 | `a_listener_on_port_zero_gets_a_port` (633) | unix | なし | 成否 | |
    | 12 | `a_second_listener_on_the_same_port_is_addr_in_use` (643) | unix | なし | 成否 | |
    | 13 | `an_accept_returns_the_connected_stream_and_its_peer` (659) | unix | なし | 成否 | |
    | 14 | `a_connect_to_a_closed_port_is_refused` (711) | unix | なし | 成否 | |
    | 15 | `a_write_to_a_closed_peer_fails_with_reset_or_broken_pipe` (732) | unix | なし | 成否 | `PEER_GONE_WRITE` |
    | 16 | `a_shutdown_write_gives_the_peer_eof` (762) | unix | なし | 成否 | |
    | 17 | `a_cancelled_accept_completes_with_an_error` (789) | unix | なし | 成否 | |
    | 18 | `a_long_write_completes_once_with_its_whole_length` (804) | unix | なし | 成否 | Windows では受け付けで終わりうるが、今のテストは両方を受け入れる |
    | 19 | `a_cancelled_pending_long_write_completes_once` (842) | unix | なし | 成否と期待値 | 準備は `write_until_pending(PENDING_CHUNK)`。取り消しの後は1回の `look` でなく `wait_for` で完了を待ち、その後に何も来ないことを確かめる。F-19 |
    | 20 | `a_cancelled_connect_does_not_try_the_next_address` (892) | unix | P14 が「保留のまま」なら なし、ほかは unix | 成否 (条件つき) | `full_listener` |
    | 21 | `quiesce_drains_a_pending_accept_and_closes_discarded_streams` (957) | unix | なし | 成否 | |
    | 22 | `a_connect_by_name_tries_the_looked_up_addresses_in_order` (1039) | unix | なし | 成否 | |
    | 23 | `a_cancelled_lookup_completes_at_once` (1064) | unix | なし | 成否 | |
    | 24 | `quiesce_does_not_wait_for_a_lookup` (1091) | unix | なし | 成否 | |
    | 25 | `live_lookups_beyond_the_limit_wait_in_order` (1116) | unix | なし | 成否 | |
    | 26 | `abandoned_lookups_do_not_hold_the_limit` (1159) | unix | なし | 成否 | |
    | 27 | `a_lookup_fails_while_64_abandoned_lookups_are_stuck` (1196) | unix | なし | 成否 | |
    | 28 | `a_read_after_a_finished_probe_returns_done` (1257) | unix | なし | 成否 | |
    | 29 | `a_cancel_after_completion_does_not_touch_the_next_op_of_the_key` (1329) | unix | なし | 成否 | |
    | 30 | `a_submit_closes_dropped_handles` (1379) | unix | なし | 成否と期待値 | 拒否を std の `TcpStream::connect` でなく driver の `Connect` (`run_op`) で確かめる。std は Windows で拒まれた SYN を約 2 秒送り直す |
    | 32 | `a_probe_on_a_stream_that_had_data_before_accept_finishes` (1503) | unix | なし | 成否 | |
    | 33 | `a_pending_read_and_a_pending_write_on_one_stream_both_finish` (1530) | unix | なし | 成否 | |
    | 34 | `queued_connections_are_accepted_one_per_op` (1569) | unix | なし | 成否 | |
    | 35 | `a_refused_connect_completes_once` (1586) | unix | なし | 成否 | |
    | 36 | `a_reset_ends_a_pending_write_with_an_error` (1610) | unix | なし | 成否 | `PEER_GONE_WRITE` |
    | 37 | `a_signal_during_a_wait_leaves_the_driver_working` (1633) | unix | unix | なし | SIGUSR1 と EINTR。Windows に待ちを割り込むものがない |

    31 番の `listen_accept_and_connect_are_unsupported` は T-3 で消えている
  - 補助の関数の `#[cfg(unix)]` を外す: `RawBuf` の import、`run_op`、`raw`、`loopback`、`listen`、`stream`、`connection`、`closed_port`、`poll_for`、`look`、`keys`、`Gate` とその impl、`PENDING_CHUNK`、`write_until_pending`、`connect_to_std`。`full_listener` は既定で `#[cfg(unix)]` のまま。P14 が「保留のまま」なら、`libc::listen(fd, 0)` の文だけを `#[cfg(unix)]` にし、Windows では std の backlog を同じループ (`connect_timeout(50 ms)` が初めて時間切れになるまで、1024 まで) で満たす
  - D-13 の定数、`WRITES_UNTIL_PENDING`、`reset_on_close` の Windows の腕。`write_until_pending` の doc の「4096 回」を「`WRITES_UNTIL_PENDING` 回」にする
  - `api.rs`: 次の9件の `#[cfg(unix)]` を外す (成否の変更): `a_real_connection_is_accepted`、`a_real_connection_carries_bytes_between_tasks`、`a_real_read_after_a_finished_probe_does_not_wait`、`a_real_read_chunk_within_keeps_the_stream`、`a_real_read_line_reads_lines_split_across_writes`、`a_real_write_of_8_mib_arrives_whole`、`an_interrupt_stops_a_run_blocked_in_a_real_accept`、`an_interrupt_stops_a_run_waiting_on_a_lookup`、`a_timeout_around_a_slow_lookup_returns_none`。補助の `REAL_CONNECTED`、`DriverCall`、`ProbeRecorder`、`block_10_s` の cfg も外す (機械的な追随)。`a_real_run_with_a_doorbell_has_a_monitor_from_its_start` は Linux のまま
  - `tls.rs`: `tests/main.rs` の gate を `cfg(all(unix, feature = "tls"))` から `cfg(feature = "tls")` にする (成否の変更)。接続先を `"127.0.0.1"` にする (期待値の変更)
  - F-19 (P8 の表示か Windows の run で、行19 の取り消した書きが `Ok(Size(全長))` で終わると分かったときだけ): 行19 の assert を全 OS で「完了はちょうど1つで、`cancelled` (`Err` "cancelled"、`cancelled == true`) か、`cancelled == false` の `Ok(Size(len))` のどちらかで、その後に何も来ない」にする (前もって認めた条件つきの期待値の変更)
  - 文書 (T-4 が持ち主のもの):
    - testing.md の 233 行: 「unix の本物の接続をまたぐタスクの読み書き」→「本物の接続をまたぐタスクの読み書き」
    - testing.md の 478 行: 「`cfg(all(unix, feature = "tls"))` の下だけで組む。」→「`cfg(feature = "tls")` の下で組む。」。「ループバックに」の所に、クライアントが `127.0.0.1` に接続し、証明書の IP の名前で照合することを足す
    - testing.md の 194 行: 「シグナルの塞ぎと handler とソケットの linger の設定のために」→「シグナルの塞ぎと handler、受けの列の長さ、ソケットの linger の設定 (unix と Windows) のために」
    - testing.md の 501 行: P14 が「すぐ拒まれる」のときだけ、一覧に「受けの列が詰まった待ち受けへの接続が進行中のまま残ること (Windows は拒む)」を足す
    - architecture.md の 377 行: 「unix のループバックの TCP の上の」→「ループバックの TCP の上の」、「シグナルで割り込まれた待ち」→「unix でシグナルに割り込まれた待ち」
    - async.md の 389 行: 「unix のものは `#[cfg(unix)]` にする。…Windows の CI は同じ組を IOCP の上で流す」→「同じ組を3つの OS で流し、Windows に無い動きを確かめるものだけを `#[cfg(unix)]` にする ([テスト戦略](../implementation/testing.md) の「Windows で流さないテスト」)」。ポート 0 と N が 2 と 8 の文は残す。引く見出しが testing.md にあることを `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` で確かめ、なければ実在する見出しを引く
    - status.md の 150 行: 本物の driver の箇条に、Windows の IOCP の backend もソケットを扱い、本物のソケットのテストを3つの OS で流すことを書く
- 実装者に任せること: コメントの文。P14 と P8 の結果に応じた行20 と F-19 の当て方は、上の条件のとおりで判断の余地はない
- 条件: いつも。行20、`full_listener`、testing.md の 501 行、F-19 は上の条件による
- 完了の条件:
  - `cargo nextest run -p eml_io --test integration -E 'test(real::)'` → `50 tests run: 50 passed`
  - `cargo nextest run -p eml_vm --test integration -E 'test(api::)'` → 30 件通る。`cargo nextest run -p eml_cli --test integration -E 'test(tls::)'` → 1 件通る
  - `cargo nextest run` → すべて通る。`cargo clippy --all-targets -- -D warnings` と `cargo fmt --check` → 警告なし
  - 3節の `.#cross` の clippy → 警告なし
  - `git grep -c 'cfg(unix)' -- crates/eml_io/tests/real.rs` → 5 (行10、行20、行37、`full_listener`、`reset_on_close` の unix の腕)。P14 が行20 を開いたら 4 (行20 の cfg が消える。`full_listener` の cfg は `libc::listen` の文に移るだけで数は変わらない)
  - `git grep -n 'cfg(unix)\|cfg(windows)' -- crates/eml_vm/tests/api.rs` → 出力なし。`git grep -n 'cfg(' -- crates/eml_cli/tests/main.rs` → `#[cfg(feature = "tls")]` だけ
  - `git grep -n -e 'ソケットを扱わないため' -e 'ソケットがないので' -e 'NotAvailable になる' -e 'Unsupported であること' -- docs/implementation docs/future/async.md crates/eml_io/tests crates/eml_vm/tests crates/eml_cli/tests` → 出力なし
  - Windows の CI が緑。ログで `grep -c 'PASS .*eml_io::integration real::'` → 47 (行20 を開いたら 48)、`'PASS .*eml_vm::integration api::'` → 29、`'PASS .*eml_cli::integration tls::'` → 1。`grep -E 'SLOW .*(real::|api::|tls::)'` → 出力なし
- 落とし穴: 拒まれた接続は Windows でも速く終わる前提 (T-3 の SYN の設定) に立つ。std の `TcpStream::connect` で拒否を確かめない

### T-5: コンソールの事象から `Terminate`

- 目的: Windows で `Terminate` を起こす OS の事象を作る
- 対象: `crates/eml_cli/src/interrupt/{windows.rs,mod.rs}`、`docs/spec/{vm.md,stdlib.md,repl.md}`、`docs/future/async.md`、`docs/implementation/{status.md,testing.md}`
- 作業: spec とテストを先に書き、次にコードを書く。付録 `terminate-prototype.diff` を当て、`mod.rs` のコメントを直す
  - テスト (`windows.rs` の `#[cfg(test)] mod tests`。Windows だけで組む):

    | テスト | 種類 | 期待 |
    |---|---|---|
    | `the_console_handler_takes_ctrl_c_and_ends_a_run_on_the_second` (既存) | 機械的な追随 (`Action` に書き換え) | 1回目の Ctrl-C → `Handled`、`stop_bits == INTERRUPT`。2回目 → `Exit(130)` |
    | `a_watched_ctrl_c_is_handled_without_exit` (既存) | 機械的な追随 | `Handled`、`stop_bits == 0`、`pending_signals == INTERRUPT` |
    | `the_console_handler_leaves_the_other_events_to_the_system` (既存) | 期待値の変更: `Mode::Run` の閉じるとシャットダウンが FALSE でなくなる | Run: Break と Logoff → `Pass`。Repl: Break、Close、Logoff、Shutdown → `Pass`。どちらも `stop_bits == 0`。`None` で Ctrl-C と Close → `Pass` |
    | `close_and_shutdown_raise_terminate_and_hold_even_when_repeated` (新規) | 新規 | 閉じるとシャットダウンのそれぞれで、新しい Run に1回目 → `Hold`、`stop_bits == TERMINATE`。2回目 → `Hold`、ビットは変わらない |
    | `a_watched_close_is_deferred_and_holds_the_handler` (新規) | 新規 | `set_watched(TERMINATE)`。閉じる → `Hold`、`stop_bits == 0`、`pending == TERMINATE`。続くシャットダウン → `Hold`、`pending == TERMINATE` |
    | `hold_never_returns` (新規) | 新規 | `act(Pass) == FALSE`、`act(Handled) == TRUE`。`act(Action::Hold)` を動かすスレッドは 50 ms 後も終わっていない (スレッドは止めたまま残す)。正しい Hold は終わらないので、遅い機械でも誤って落ちない。`act(Exit)` は呼ばない |

  - 文書 (日本語は natural-japanese で整える。引用の文は今の文):
    - vm.md の「上限と割り込み」:
      - 「SIGTERM のハンドラは `eml run` だけが入れる。`eml repl` のプロンプトでは止める語を読む者がいないので、SIGTERM は OS の既定の動作でプロセスを終わらせる。」→「SIGTERM のハンドラは `eml run` だけが入れる。Windows の handler はどちらでも入れるが、閉じることとシャットダウンを受けるのは `eml run` だけである。`eml repl` のプロンプトでは止める語を読む者がいないので、SIGTERM と Windows の閉じることとシャットダウンは、OS の既定の動作でプロセスを終わらせる。」
      - 「読み込みとコンパイルの間の Ctrl-C と SIGTERM も」→「読み込みとコンパイルの間の Ctrl-C と `Terminate` のシグナルも」
      - 「Windows の handler は、OS が作るスレッドで」で始まる箇条を次の内容にする: handler は OS が作るスレッドで動く。`CTRL_C_EVENT` は `INTERRUPT` として下の手順で扱い、TRUE を返す。`eml run` では `CTRL_CLOSE_EVENT` (コンソールを閉じたときと、タスク マネージャーでタスクを終了したとき) と `CTRL_SHUTDOWN_EVENT` を `TERMINATE` として扱う。この2つでは handler が返るとすぐに OS がプロセスを終わらせるので、handler は手順2か3と4を行った後、返らずにスレッドを止めておく。プロセスは、プログラムが普通の道で終わるか、OS の待ち時間 (閉じるは既定で 5 秒、シャットダウンは既定で 5 秒、サービスでは 20 秒) が切れたときに終わる。待ち時間が切れたときは、まだ書いていない出力は失われ、終了コードは OS が決める。続けて来た閉じるとシャットダウンも手順1に進まず、同じく返らずに待つ。どちらも人が送り直すものでなく、安全点に来ない処理からの出口は OS の待ち時間だからである。`CTRL_BREAK_EVENT` と `CTRL_LOGOFF_EVENT`、`eml repl` での閉じるとシャットダウンには FALSE を返し、OS の既定の handler がプロセスを終わらせる。このとき、まだ書いていない出力は捨てる。ログオフを写さないのは、それを受けるのが主にサービスで、サービスにはどの利用者のログオフでも届くためである。Ctrl-Break は、REPL が抜けられない処理から抜ける道である ([REPL の端末](repl.md) の「残る制限」)
      - 「ハンドラは static に置いた」の箇条の末尾に「Windows の閉じるとシャットダウンでは、その後スレッドを止めて返らない。止めたスレッドはロックを持たない」を足す
      - 手順の前置きの「(Ctrl-C は `INTERRUPT`、SIGTERM は `TERMINATE`)」→「(Ctrl-C は `INTERRUPT`、SIGTERM と Windows の閉じることとシャットダウンは `TERMINATE`)」
      - 手順1の末尾に「Windows の閉じることとシャットダウンはこの手順を持たない (上)」を足す
      - 終わりの段落の「SIGTERM で止まったとき (`RuntimeError::terminated`) は 143」に「(Windows の閉じることとシャットダウンも同じ)」を足す
    - stdlib.md の「`Std.Signal`」:
      - 「`Interrupt` は Ctrl-C で」で始まる箇条を次にする: `Interrupt` は Ctrl-C で、unix の SIGINT と Windows の `CTRL_C_EVENT` である。`Terminate` は unix の SIGTERM と、Windows の `CTRL_CLOSE_EVENT` と `CTRL_SHUTDOWN_EVENT` で、どれも `eml run` だけが受ける。`CTRL_LOGOFF_EVENT` は写さない。Windows では、`Terminate` を受けたプログラムは OS の待ち時間 (閉じるは既定で 5 秒) のうちに終わらなければならない。OS がシャットダウンを送るのはサービスなど一部のプロセスだけである ([VM](vm.md) の「上限と割り込み」)
      - 「Ctrl-C は `_exit(130)`、SIGTERM は `_exit(143)` である」→「unix では Ctrl-C は `_exit(130)`、SIGTERM は `_exit(143)` で、Windows の Ctrl-C は `TerminateProcess` で 130 である。Windows の閉じることとシャットダウンは続けて来ても終わらせず、OS の待ち時間に任せる」
      - 「Windows と `eml repl` の `Terminate` の待ちは」から「Windows と」を消す
      - 「本物のシグナル (Ctrl-C と SIGTERM)」の括弧を「(Ctrl-C、SIGTERM、Windows の閉じることとシャットダウン)」にする
      - 「`eml repl` は SIGTERM を受けず、…」の箇条を「`eml repl` は SIGTERM も Windows の閉じることとシャットダウンも受けず、OS の既定の動作でプロセスが終わる。`Signal.wait Interrupt` は REPL でも使える」にする
      - 箇条を足す: Windows でコンソールを閉じた後は、コンソールへの書き込みと読みが失敗しうる。そのときは先に起きた誤りで run が終わる (書き込みの誤りなら終了コード 1)。出力をファイルか pipe に向けていれば、`Terminate` の後も 143 かプログラムの終了コードで終わる
    - repl.md の 263 行 (「REPL は SIGTERM のハンドラを入れない」) の後に「Windows でも、閉じることとシャットダウンは OS の既定の handler に任せる。`Signal.wait Terminate` を起こす OS の出来事は、REPL ではどの OS にもない」を足す
    - async.md: 「Windows には `Terminate` を起こす OS の出来事がない。…」の箇条を消す。`drain_within` の例の段落に「Windows でコンソールを閉じたときは OS が既定で 5 秒で終わらせるので、30 秒は待てない」を足す
    - status.md の 150 行: 「Windows には `Terminate` を起こす OS の出来事がない。」と、末尾の「、[ロードマップ](../future/roadmap.md) の `net-windows`」を消す。箇条を足す: Windows の `eml` は `rustyline` を通して user32.dll を読み込むので、Windows 10 までは OS がログオフとシャットダウンの事象を送らず、`Terminate` を起こすのはコンソールを閉じることだけである。本物の閉じる事象を送るテストはなく、handler の判断の関数と返らないことを単体テストで確かめる
    - testing.md の「シグナルのハンドラは、bin の `interrupt` モジュールの単体テストが確かめる」の段落の Windows の文を次の内容にする: Windows では `windows.rs` のテストが handler の判断の関数を直接呼ぶ。`CTRL_C_EVENT` は TRUE の答えで `INTERRUPT` を立て (待つタスクがいれば保留にし)、`eml run` の2回目は 130 で終わる答えになる。`eml run` の `CTRL_CLOSE_EVENT` と `CTRL_SHUTDOWN_EVENT` は返らない答えで `TERMINATE` を立てるか保留にし、2回目も返らない答えのままビットを変えない。ほかの事象、REPL の閉じるとシャットダウン、static を置く前の事象は FALSE の答えになる。返らない答えを行う関数は、別のスレッドで動かして 50 ms 後もまだ終わっていないことを確かめる。Windows で本当の Ctrl-C や閉じる事象を送るテストは置かない
- 実装者に任せること: コメントの文
- 条件: いつも
- 完了の条件:
  - 3節の `.#cross` の clippy → 警告なし
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 警告なし (Linux の `cargo fmt --check` は `cfg(windows)` のファイルも見る)
  - `cargo nextest run -p eml_cli` → 通る
  - Windows の CI が緑で、ログに `interrupt::windows::tests::` の上の6件の PASS がある。`eml_vm` の `signal::` も通る
  - ``grep -rn '`Terminate` を起こす OS の出来事がない' docs/`` → 出力なし。``grep -rn 'Windows と `eml repl` の `Terminate`' docs/`` → 出力なし
  - `grep -c CTRL_CLOSE_EVENT docs/spec/stdlib.md docs/spec/vm.md` → どちらも 1 以上
- 落とし穴: `act(Exit)` をテストで呼ぶとテストのプロセスが終わる。ログオフを写さない (D-14)

### T-6: 段を閉じる

- 目的: roadmap の「進め方」の閉じる手順を1つのコミット (`Close net-windows`) で行う
- 対象: `docs/spec/`、`docs/implementation/{benchmarks.md,status.md,architecture.md}`、`docs/future/roadmap.md`、`docs/overview.md`、`docs/README.md`、`CLAUDE.md`、`docs/plans/2026-10-31-net-windows*`
- 作業: roadmap の手順1から9に従う。この段に固有の点は次のとおり
  - 手順2 (計測): `crates/eml_vm` は `tests/` だけが変わったが、規則の対象のパスに入るので計測の対象とする。macOS がなければ表を入れ替えず、`Measure net-windows` をユーザーに頼む (Q-3)
  - 手順5: roadmap から `net-windows` の節を消す。後に回した項目はない
  - 手順8: `docs/plans/2026-10-31-net-windows.md` と付録のディレクトリを消す。消す前に、D-1 から D-5、D-11 から D-14 の規則が spec か実装の文書にあることを確かめる
  - 手順9: コミットのメッセージに、テストの変更の種類と理由 (T-2 から T-5 で書いたもの)、IPv6 を確かめたかどうか、試しで当てた代わりを書く
- 実装者に任せること: overview.md と README.md の表の文
- 条件: いつも
- 完了の条件:
  - `grep -rn 'net-windows' docs crates CLAUDE.md` → 出力なし
  - `ls docs/plans/2026-10-31-net-windows* 2>/dev/null` → 出力なし
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → 通る
  - `cargo nextest run` → すべて通る
- 落とし穴: 段の名前と「今は〜、後で〜」の予告を spec に残さない

## 8. 実行の順

T-1 → T-2 → T-3 → T-4 → T-5 → T-6 の一本道で、並行はしない。T-5 は内容では独立しているが、status.md の 150 行の受け渡し (D-15) のため T-4 の後に置く。各タスクは Windows の CI が緑になってから次に進む (D-20)。

## 9. 確認の計画

- 基点での確認 (最初の変更の前): `cargo nextest run` → すべて通る。`cargo clippy --all-targets -- -D warnings` → 警告なし。3節の `.#cross` の clippy → 警告なし
- 全体の確認 (すべてのタスクの後):
  - `cargo nextest run` → すべて通る。`cargo clippy --all-targets -- -D warnings && cargo fmt --check` → 警告なし
  - 3節の `.#cross` の clippy を `x86_64-pc-windows-msvc`、`x86_64-unknown-linux-gnu`、`aarch64-unknown-linux-gnu` で → 警告なし
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib -- driver::tests::` → 通る (`-Zmiri-tree-borrows` を足しても通る)
  - CI の全 job (Linux 2つ、macOS、Windows、Verus、Miri、ASan) が緑。Windows の job で T-1、T-3、T-4、T-5 の名前と数の確認が通る
- ユーザーの手での確認:
  - Windows の機械で `eml run` のプログラムを動かしたコンソールを閉じ、`Signal.wait Terminate` を待つプログラムが自分の終了コードで終わり、待たないプログラムが 143 で終わることを確かめる (出力はファイルに向ける)。D-14 の結果を確かめる。CI では閉じる事象を送れない
  - 余裕があれば `dumpbin /imports eml.exe | findstr /i user32` で user32 の import を確かめる (status.md の記述の裏付け)

## 10. 危険と未決の項目

| ID | 項目 | 影響 | いつ誰が決めるか | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | 段のブランチを force-push して、緑になるまでタスクのコミットを直してよいか | T-1 から T-5 | 決定済み: ユーザーは認めたが、この環境は force-push を拒む。段のブランチ `claude/happy-ptolemy-u2f1ko` に push して `workflow_dispatch` で CI を起こし、赤なら修正のコミットを重ねる (4節の「ないとき」の形) | - |
| Q-2 | T-1 の試しのどれかが「止める」になる | T-2 以降すべて | T-1 の CI の結果を見てユーザーと計画を見直す | 段を止める |
| Q-3 | 段を閉じる計測をどうするか (macOS がない。eml_vm はテストだけが変わる) | T-6 | 決定済み: 表は入れ替えず、`Measure net-windows` をユーザーが macOS で行う | - |
| Q-4 | runner に IPv6 のループバックがない | P11 と backend のテスト8 | T-1 の CI の結果 | `::1` の回を外し、IPv6 は確かめていないと記録する |
| Q-5 | 満ちた受けの列への接続の振る舞い | T-4 の行20 と testing.md の 501 行 | P14 の表示 | 行20 は unix のまま、testing.md の行は足さない |
| Q-6 | 頼んでいない中断の原因は LSP しか知られていない | D-12 の `Other` | 実際に見えたとき | `Other` のまま |
| Q-7 | backend の `Drop` の abort のテストがない | 核の誤りでしか届かない | なし | 置かない |
| Q-8 | 保留の `ConnectEx` を取り消す本物のソケットのテストがない (Windows で接続を保留のまま保つ決まった方法がない) | 取り消しの道の確かめ | P14 が「保留のまま」なら T-4 の行20 で確かめる | 核の偽の backend のテストだけで確かめる |
| R-1 | backend の `unsafe` を Windows の Miri と ASan で確かめられない | 記憶の安全 | なし | D-11 の番地の規則とテストとレビューで補う |

## 付録: レビューで変わった主な点

- `quiesce` で取り消しが1回断られると、残りの欄に OS の取り消しが出ないまま abort していた → `Drop` を `cancel_sent` で選ぶように直した (D-3)。偽の backend の試しで再現と修正を確かめた
- 待ちの失敗で壊れた driver をすぐ abort していた → roadmap の前提どおり、取り消しを出して 1 秒まで読む形にそろえ、`By` を消した (D-3)
- 境界のコミットで dead_code の印を変えると Windows の clippy が落ちる → 印は T-3 で変える (D-5、G-10)
- packet の番地を確かめずに `Box::from_raw` していた → 番地の表で先に確かめる (D-11)
- `Accept` の後処理の失敗をすべて出し直していた → 接続ごとの誤りだけを出し直す (D-12)
- 試しに経過時間の assert があった → 1 秒の待ちを見張りにし、経過時間は表示だけにした (D-10)
- `tls.rs` が `localhost` に接続していて `::1` に頼っていた → `127.0.0.1` にした (D-13)
- 2回目の閉じるで 143 にしていた → OS が送る事象なので Hold を続け、OS の待ち時間に任せる (D-14)
- trust.rs の T5 の許す一覧が `driver/tests/mod.rs` の `mod abort;` を落としていた。grep が `from_raw_fd` などに当たっていた → 一覧に足し、grep を `-w` にした (T-2)
- T-2 の vm.md の文が、まだない IOCP の振る舞いを書いていた → IOCP に固有の文は T-3 に移した (D-15)
