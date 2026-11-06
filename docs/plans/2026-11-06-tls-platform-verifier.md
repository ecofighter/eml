# `Std.Tls`の証明書をOSの検証器で確かめる段 (tls-platform-verifier)

- 作成日: 2026-11-06
- 基準: main @ b80f06e7 (この文書の「背景」と、`ファイル:行`の参照はこのコミットの状態を指す)
- 状態: 計画は確定。10の未解決の項目には、どれも既定の扱いを決めてある

## 1. 目的と完了の条件

- 目的: `Std.Tls`は、サーバーの証明書をwebpki-rootsの根ではなくOSの検証器 (`rustls-platform-verifier`) で確かめ、webpki-rootsを依存から外す。`Tls.connect_with_ca`と`Tls.wrap_with_ca`に渡したCAは、OSの根に加えて信頼する。OSの検証器はブロックしうる (trustdとのIPC、失効やAIAの取得、Windowsでは最大で約20秒) ので、検証しうる処理 (`Conn`を作ることと、handshakeの途中の押し込み) はdriverのブロックする呼び出しのプールで動かす。そのために、`Lin`の`Foreign`の値を値で移してRustの処理をプールで動かす汎用の仕組みを作る。待ち、取り消し、期限は`Std.Net`に残す
- 完了の条件: 7のT-0〜T-10をそれぞれ1コミットで入れ、各作業の完了の条件と9の全体の検証がすべて期待どおりになること

## 2. 範囲

- 範囲に入るもの: `eml_rt` (`Heap::foreign_take`)、`eml_vm` (汎用の仕事の記録、`jobs.rs`、TLSの行とその振り分け)、`eml_extern` (TLSの行)、`std/Tls.em`、`eml_tls` (`Name`、`Trust`、rustls-platform-verifierへの切り替え、fixtures)、`eml_io`と`eml_cli`のテスト、`bench/scripts.sh`と`crates/eml_tls/examples/https_server.rs`、`Cargo.toml`、`Cargo.lock`、`about.toml`、`THIRD-PARTY-LICENSES.md`、各作業に挙げた文書
- 範囲に入らないもの: stdの`extern`の行に付け、translateが下ろす`blocking`の属性 (閉じるときにロードマップの「その後の項目」へ移す。D-1)、サーバーの側のTLSのAPI、`Std.Tls`の期限を取る読み、暗号のproviderの変更 (aws-lc-rsのまま)

## 3. 背景

- 証明書を確かめる所は`Conn::push_tls`から呼ぶ`process_new_packets` (`crates/eml_tls/src/lib.rs:144`) だけで、それを呼ぶのは`Std.Tls.push_tls`のextern (`crates/eml_vm/src/tls.rs:171`、`vm.rs:4251`) だけである。VMのworkerの上で同期して動く。rustlsは検証器をその場で呼び (rustls 0.23.46の`client/tls13.rs:1192`、`client/tls12.rs:885`)、検証を後に回す仕組みを持たない
- `Op::Job`を作るのはブロックするCの呼び出し (`crates/eml_vm/src/ffi.rs:187`) だけである。VMは`Done::Job`の中身として`u64`しか受けない (`crates/eml_vm/src/cells.rs:3564-3573`)。`eml_rt`には`Foreign`の箱から値を取り出すAPIがない
- `Conn`は`Lin`の`Foreign`である (`TlsConn { conn, line_from }`、`tls.rs:84-97`。`tls.rs:158`の`alloc_foreign(_, true)`)
- テストのfixtures (`crates/eml_tls/fixtures/`) の有効期間は2026-01-01〜2126-01-01である。macOSでrustls-platform-verifier 0.7.1を使って測った結果: CAを追加の根として渡してもfixturesは拒まれる (`-67901`)。自前の根の下では、有効期間825日のleafは通り、826日は拒まれる。`ClientConfig`で固定した検証の時刻はOSまで届く
- rustls-platform-verifier 0.7.1 (2026-09-24、MSRV 1.85、rustls ^0.23.27。`Cargo.lock`は0.23.46): Linuxでは作るときに根のファイルを読み (rustls-native-certs。`SSL_CERT_FILE`と`SSL_CERT_DIR`)、システムの根も追加の根もなければ`General("No CA certificates were loaded from the system")`で失敗する。Windowsでは、システムの鎖が失敗したとき (鎖の途中まで、または信頼しない根) だけ追加の根を試し、鎖を1つ組むたびにURLの取得に累計10秒の上限がある。macOSでは`SecTrustEvaluateWithError`を呼び、ネットワークの制限を付けない
- 今、既定の根がfixturesのCAを拒むことに頼るテストは`tests/ui/run-fail/tls/unknown_ca.em`、`crates/eml_tls/tests/conn.rs::an_unknown_ca_is_rejected_with_a_fixed_message`、`crates/eml_io/tests/tls.rs::a_rejected_certificate_closes_the_plaintext_end`である。ほかのTLSとHTTPのUIテストはすべて`ca.pem`を渡す。本物のソケットのTLSのテストは`crates/eml_cli/tests/tls.rs::a_client_talks_to_a_local_tls_server`だけである
- 付録のディレクトリ`docs/plans/2026-11-06-tls-platform-verifier/`に、最終の設計 (英語) と試作がある
  - `01-pool-jobs.md`: Rustの処理をプールで動かす汎用の仕組み (長期の方針。適合の基準G-1〜G-13)
  - `02-verifier.md`: `eml_tls`のAPI、どこでどの検証器を使うか、fixtures、テスト、CI、依存、文書
  - `03-tls-on-pool.md`: どのTLSの処理をプールで動かすか、行、`std/Tls.em`の変更、取り消し、テスト、specの文
  - `patches/01-foreign-take.patch`: T-1の`eml_rt`の部分。gateを通してある (`cargo verus verify -p eml_rt`が1201 verified、0 errors。新しいtrustedのテストが両方のmodeで通る)。基準のコミットに`git apply`で当たる
  - `patches/02-verifier-prototype.patch`、`patches/03-tls-on-pool-prototype.patch`: 試作。参考であり、写すコードではない。02は`client_raw`の中でテストの世界の根を決め打ちにしている。03は仕事の値を代わりの道 (`OpKind::Ffi`の上の`proto_submit`と`proto_take`) で運び、デバッグの`eprintln!`と仮の`Roots`を含む。どのパッチも基準のコミットに単独で当たる
- 付録どうしが食い違うときは、次の順で先にあるものに従う
  1. この文書 (5の決定、7の作業、利用者の答え)
  2. `01-pool-jobs.md`の8 (適合の基準。直した文のもの)
  3. VMの側の振り分けのインターフェース (`Roots`、`client_conn`) は`03-tls-on-pool.md`の5.1。`eml_tls`のAPI、fixtures、テスト、CI、依存は`02-verifier.md`
  4. プールの行、`std/Tls.em`、プールの仕事についてのTLSのspecの文は`03-tls-on-pool.md`
- 付録の中のF1、S1、S2、S2-A、S1a、U1、G-4のような名前は、計画の中だけの呼び名である。コードと`docs/`には書かない

## 4. 前提の環境

| 要るもの | 使う所 | 確かめ方 | ないとき |
|---|---|---|---|
| Nixのdevshell (cargo、cargo-nextest、cargo-insta、clippy) | 全作業 | `cargo nextest --version` → 版が出る | 止めて利用者に聞く |
| devshellのVerusとverusfmt | T-1 | `which cargo-verus` → パスが出る。`verusfmt --version` → 版が出る | 止めて利用者に聞く |
| `.#miri`のshell | T-1 | `nix develop .#miri -c cargo miri --version` → 版が出る | 止めて利用者に聞く |
| `plamo-translate` (MCPの道具かCLI)。日本語の文をすべてこれで訳す (`CLAUDE.md`) | 文書かコメントを書く全作業 | MCPの道具`translate`が一覧にある、または`plamo-translate --help`が0で終わる | `natural-japanese:natural-japanese`のスキルに従う (`CLAUDE.md`) |
| nixのOpenSSL 3.4以上 (`gen.sh`のため) | T-4 | `nix shell nixpkgs#openssl -c openssl version` → `OpenSSL 3.4`以上 | 止めて利用者に聞く |
| `/usr/bin/openssl`とPython 3のあるmacOS | T-5 (bench)、T-10 (計測) | `uname` → `Darwin`。`/usr/bin/openssl version` → LibreSSLが出る | T-10の計測を別のコミット`Measure tls-platform-verifier`にする |
| crates.ioへのネットワーク | T-8 | 依存を足した後の`cargo fetch`が0で終わる | 止めて利用者に聞く |
| リポジトリのCIを読める`gh` | T-8 (Windowsの時間の確かめ) | `gh run list -L 1` → 実行が1つ出る | 利用者がCIの実行を手で確かめる |

## 5. 決定

### 5.1 長期の方針 (変えるには検討し直しが要る)

実装の途中でこれと食い違う所が出たら、止めて報告する。

#### D-1: `Lin`の`Foreign`を移して、Rustの処理をブロックする呼び出しのプールで動かす

- 決定: `01-pool-jobs.md`の1。要点は次のとおり
  - 記録の種類は`OpKind::Job { from: JobSource, answer: Option<Box<dyn Any + Send>> }`の1つで、`JobSource { C(u32), Std(Extern) }`を持つ。完了の腕は箱を見ずにそのまま持ち、仕事を出した行の取り出しが箱をdowncastする
  - `crates/eml_vm/src/jobs.rs`の`submit` (クロージャを`catch_unwind`で包み、箱は`Result<T, String>`を持つ) と、`Outcome { Woke(T), Failed(io::Error), Cancelled }`を返す`take`
  - `Lin`の`Foreign`の入力は、新しい`Heap::foreign_take::<T>`で取り出す (vtableを何もしないものに替えるので、普通の`decref`が破棄処理を走らせずに箱を放す)。クロージャが捕まえて値で返し、取り出しがタスク自身のヒープに`alloc_foreign(_, true)`で箱を作り直す。出す行は、`foreign_take`、`decref`、`Unr`の入力の写し、`jobs::submit`の順に行う
  - emlの側は、手で書く3つ組`let (w, op) = Async.io_wait (submit_x ..)`と、その次の`take_x op w`である
  - `Cancelled`のとき、取り出しは値を落とし、`Cancelled`の`task was cancelled`を投げる
  - ブロックするCの呼び出しも、この段で同じ記録 (`JobSource::C`) を使う。下ろし方と`Op::FfiSubmit`、`Op::FfiTake`は残す
- 理由: 仕事はヒープへのポインタを借りずに自分の値を持つので、プールはヒープに触れず、記録はヒープの語を持たず、checked modeとworkerごとのヒープに新しい規則が要らない。記録の種類が1つなので、`cells`に型ごとの変種が要らない。操作を待つタスクは救済されないので、取り出しは出したworkerの上で動く
- 退けた案: 待っている継続が生かす`*mut T`を貸す (値で返す前提と食い違い、T16を広げる)。取り出して同じ箱に書き戻す (`eml_rt`の操作が2つと、穴の空いた状態が要る)。Rustの型ごとの`OpKind`の変種。stdの行に付け、translateが下ろす`blocking`の属性 (後に回す。`01-pool-jobs.md`のU1)
- 影響の範囲: Rustの処理をプールで動かすstdのexternの行すべて (今は`Std.Tls`、今後のそうした行も)。ブロックするCの呼び出し
- 見直す条件: 操作を待つタスクが別のworkerへ移る段が来たとき (取り出しは`with_worker`の下で箱を作り、印を付ける必要がある。G-13)。Rustの仕事の行の3つ目の族が現れたとき (U1を見直す)
- 適合の確かめ: `01-pool-jobs.md`の8のG-1〜G-13。それぞれに確かめのコマンドがある。G-4、G-5、G-9と、5の取り出しの失敗の箇条は、利用者が採った直した文である (元の確かめは、書いたとおりでは通りえなかった)
- リポジトリに残す所: `docs/spec/vm.md`の「プールの仕事」(新しい節) と「`blocking`の呼び出し」、`docs/spec/object-model.md` (`foreign_take`)、`docs/spec/stdlib.md`の「extern の境界」。T-1〜T-3が書く

### 5.2 短中期の設計 (理由があれば実装中に変えてよい。変えたらコミットのメッセージに書く)

#### D-10: `eml_tls`のAPIと検証器

- 決定: `02-verifier.md`の5.1。`Name::parse`。`Trust` (`Clone + Send + Sync + 'static`) と、その`platform(extra_ca_pem)`、`platform_at(extra_ca_pem, now)`、`fixtures(ca_pem)`。`Conn::client(&Name, &Trust)`はI/Oをしない。新しい`Error::NoRoots`の文言は`no root certificates are available`である。追加のPEMは、検証器を作る前に`eml_tls`が確かめる (`CaPem`はどのOSでも同じになる)。`Opaque`の包みは、`InvalidCertificate`でない検証器の誤りをすべて拒否に変え、OSの文言を文言に届かせない。`ClientConfig`は接続ごとに作る。プロセス全体のcacheは持たない
- 理由: 失敗の文言を固定する (ロードマップの項目)。接続の設定の誤りの順 (ServerName、CaPem、NoRoots、Config) を保つ。大域の状態を持たない (Linuxで作るのにかかる0.7〜1.6 msは、handshakeとほぼ同じである)
- 退けた案: プロセス全体の検証器のcache。`BuilderVerifierExt`と`ConfigVerifierExt` (追加の根も時刻の差し込みもない)

#### D-11: どの検証器を使うか、テストのfixtures

- 決定: 新しい設定は足さない。`Machine`が自分でテストのdriverを作ったrun (`DriverKind::Virtual`) は`Trust::fixtures`で確かめる。これは、渡したCAだけを根にしたrustlsのWebPKIを、`fixtures::NOW` (2026-01-15T00:00:00Z) の時刻で使う。本物のdriverか、差し込んだdriverのrunは、いつも`Trust::platform`を使う。fixturesは期間2026-01-01〜2026-02-01で1度だけ作り直し、拒否のテストのための2つ目のCA`other-ca.pem`を足す。この後は作り直す必要がなく、現実の時刻ではいつも期限切れである
- 理由: `CLAUDE.md`はテストのためだけの欄を禁じる。弱い検証器は、ネットワークが偽物の所でしか使えない。時刻を固定すれば、どのOSでも日付に左右されない
- 退けた案: `RunConfig.tls_verify`の欄 (本番のAPIに置くテストのためだけの欄)。開発のためだけのcargoのfeature (featureの統合で本番の道が隠れる)。期限の短いfixturesを定期的に作り直す (結果が日付に左右される)。テストの世界全体で、固定した時刻のOSの検証器を使う (約20のテストでWindowsの鎖を組むことになり、結果がOSに左右される)

#### D-12: どのTLSの処理をプールで動かすか

- 決定: `03-tls-on-pool.md`の1と5.1〜5.3。`Conn`を作ること (名前、根、`Conn::client`) と、`HANDSHAKING`が立っている間の押し込みはすべてプールで動かす。handshakeの後の押し込みとほかの行は、すべて同期のままにする。`client_raw`をなくし、`submit_client : String -> String -> <IO> Async.OpId`、`submit_push : Bytes -> Conn -> <IO> Async.OpId`と、両方で使う`take_conn : Async.OpId -> Async.Wake -> <IO> Conn`を入れる (中身は`Result<TlsConn, String>`、`JobSource::Std(Extern::TlsTakeConn)`)。`fill`は、呼び手が読んだ状態の`HANDSHAKING`のビットで押し込みを選ぶ。`push_tls`の腕は、`HANDSHAKING`が立っていれば`VmFault::Internal("Std.Tls.push_tls runs during the handshake")`を返す。ここに届くのはstdの誤りだけである (rustls 0.23.46では、handshakeの2つの印はfalseからtrueへしか変わらない。TLS 1.2のHelloRequestは断り、TLS 1.3の通信の後に受けるのはNewSessionTicketとKeyUpdateだけで、どちらも検証しない)
- 理由: handshakeの後も押し込みのたびにプールを往復すると、benchの`https-get`の行で約9600の仕事になる。handshakeだけなら、1つの接続につき設定の仕事1つと押し込みの仕事1つで済む
- 退けた案: すべての押し込みをプールで動かす。handshakeの1歩を進める別の行

#### D-13: VMの側の振り分けのインターフェース (T-4とT-6が共有する)

- 決定: `03-tls-on-pool.md`の5.1をそのまま使う。`crates/eml_vm/src/tls.rs`に`enum Roots { Fixtures, Platform }`、`Roots::of(DriverKind)` (`Virtual`なら`Fixtures`、ほかは`Platform`)、非公開の`Roots::trust(pem)`、`client_conn(roots, host: &str, pem: &[u8]) -> Result<TlsConn, String>`を置く。文言は`cannot set up TLS for {host}: {e}`で、feature `tls`がなければ`…: not available`である。T-4が作り、同期のままの`client_raw`から呼ぶ。`client_raw`は`Extern::is_async`と`async_op`へ移す (`run_extern`はschedulerを持たないので、`sched.driver_kind`が読めるのは`async_op`だけである。`vm.rs:3286`。`vm.rs:4635`の2つ目の`Async`のexternの一覧にも腕を足す)。T-6は呼び出しをclientの仕事へ移すだけで、振り分けは書き直さない
- 理由: 作り直したfixturesは時刻を固定した検証器なしには使えないので、T-4ですぐに振り分けが要る

#### D-14: TLSの仕事の間の取り消し、期限、割り込み

- 決定: `03-tls-on-pool.md`の5.4。走っている仕事は最後まで走る。その後、取り消しか期限が`Conn`とソケットをclose_notifyなしに落とし、`Cancelled`を投げる。`Async.timeout`は仕事が終わってから戻る。接続の設定は取り消しの点である。仮想の時計では、期限が仕事の途中に来ることはない。長い検証の途中の割り込みか`exit`は、仕事の予算 (1秒) まで待ち、それを過ぎると仕事を見捨てて`Machine`をpoisonedにする (REPLはそのセッションを終える)。TLSの仕事は、`blocking`の呼び出しと24の`Op::Job`の枠を分け合う。仕事を走らせられないときは`cannot run TLS: {e}`を投げる (行の前置きは固定の文字列`cannot run TLS`である)

#### D-15: OSの検証器のテストとCI

- 決定: `02-verifier.md`の1.5〜1.7と5.5。本物のOSの検証器に届くテストは次のとおりである。`crates/eml_tls/tests/platform.rs` (すべてのOSで`NOW`の時刻の3つのテストと、Linuxだけの2つのテスト。後者は`SSL_CERT_FILE=/dev/null`にして`SSL_CERT_DIR`を外し、テストのバイナリを子として流し直して、nixのsandboxの代わりにする)。本物のソケットの`crates/eml_cli/tests/tls.rs` (`other-ca.pem`による拒否のテストにする。どの日付でも成り立つ)。OSの根に頼るテストは置かない。Windowsの残る不明 (根の自動更新) は、rustls-platform-verifier自身のwindows-2025のCI (URLのない証明書で27のテストが0.17秒) を根拠に、既定では受け入れる。だめだったときの扱いはQ-1にある
- 本物のソケットのテストの名前は`a_real_socket_handshake_with_an_unknown_ca_is_rejected`にする (T-4では拒否はまだwebpki-rootsから来て、T-8からはOSの検証器から来る)

## 6. 実装の制約と決まり

- `cells.rs`、`jobs.rs`、TLSのVMの腕、`std/Tls.em`、`eml_rt`、UIテストに触る作業は、D-1の適合の基準に従う (D-1)
- `eml_rt`の変更は`crates/eml_rt/CLAUDE.md`のgateを通す (D-1)
- `docs/spec/`は規則を実装するコミットで書き、コードのコメントはspecの節を引く。`docs/plans/`は引かない (`CLAUDE.md`)
- 日本語の文 (文書、コードのコメント、UIテストの頭のコメント) は英語で下書きし、PLaMo (`form: plain`) で訳す。日本語とラテン文字やインラインコードの間に空白を入れない。直す段落にそうした空白があれば消す (`CLAUDE.md`)
- 文書を直す時期はロードマップの「進め方」に従う。`docs/spec/`、`docs/implementation/testing.md`、`docs/implementation/benchmarks.md`の文は、その事実を変えるコードと同じコミットで直す。`docs/implementation/status.md`、`architecture.md`、`docs/overview.md`、`docs/README.md`、`docs/future/roadmap.md`、`CLAUDE.md`は閉じるコミットT-10で直す
- 失敗の文言はどれも固定のままにし、OSとrustlsの詳細を付けない (D-10)
- 試作の`crates/eml_tls/examples/probe.rs`はコミットしない。fixturesは試作のPEMを写さず、`gen.sh`で作り直す (D-11)
- その場で流す`.em`のプログラムは`timeout 20`の下で動かす (`CLAUDE.md`)

## 7. 実装の作業

テストの変更を、[テスト戦略](../implementation/testing.md)の「テストの変更の運用」の種類で挙げる。種類1と2は、この計画の承認が合意になる。

| 種類 | 作業 | テストの変更と理由 |
|---|---|---|
| 1 | T-4 | `crates/eml_cli/tests/tls.rs::a_client_talks_to_a_local_tls_server`を消す。本物のdriverでは、コミットした証明書が現実の時刻で有効であり続けることはない。D-15の拒否のテストに置き換える |
| 2 | T-2 | `crates/eml_vm/src/cells.rs`の`a_job_answer_of_another_type_is_an_internal_error` (`cells.rs:6348`)。完了は成功するようになり、内部の誤りは`take_job(..).answer::<u64>()`へ移る |
| 2 | T-4 | 範囲として: fixturesのデータ (`crates/eml_tls/fixtures/{ca,cert,key}.pem`と4つの`ca.pem`の写し) を作り直す。テストのdriverの上のTLSはすべて`Trust::fixtures`で確かめるので、CAを渡さない`Tls.connect`と`wrap`は接続の設定で`NoRoots`になる。個別に: `unknown_ca.em` (入力を`other-ca.pem`に。snapshotの位置は19:7から20:7)、`server_closes_before_handshake.em` (`ca.pem`を渡す。21:7から22:7)、`conn.rs::an_unknown_ca_is_rejected_with_a_fixed_message`と`eml_io/tests/tls.rs::a_rejected_certificate_closes_the_plaintext_end` (入力を`Trust::fixtures(OTHER_CA_PEM)`に)、`conn.rs::setup_errors`、`crates/eml_tls/tests/provider.rs`、`eml_vm`の`tls::on_tests::client_errors_are_other_with_fixed_messages` (`02-verifier.md`の5.5) |
| 2 | T-6、T-7 | `crates/eml_core_ir/tests/externs.rs`。印と借用の表、doc commentの数を新しい行に合わせる (`03-tls-on-pool.md`の5.5) |
| 3 | T-2、T-4 | T-2: `a_job_record_takes_the_boxed_answer` (6292) と`a_cancelled_task_submits_no_job` (6322) の名前の追随。T-4: `conn.rs`と`eml_io/tests/tls.rs`の`client()`の助け、`on_tests`の`new_client`と`pump` |

| ID | 作業 | 基づく決定 | 前提 | 並行 |
|---|---|---|---|---|
| T-0 | 計画を入れる | - | - | 不可 |
| T-1 | `eml_rt`: `Heap::foreign_take` | D-1 | T-0 | 可 (T-4、T-5と並べて準備できる。コミットの順は8のとおり) |
| T-2 | `eml_vm`: 汎用の仕事の記録。Cの呼び出しをその使い手にする | D-1 | T-1 | 不可 |
| T-3 | `eml_vm`: `jobs.rs`とプールの仕事のspec | D-1 | T-2 | 不可 |
| T-4 | 時刻を固定したfixtures、`Name`と`Trust`、VMの側の振り分け | D-10、D-11、D-13、D-15 | T-0 | 可 (T-1〜T-3と独立) |
| T-5 | bench: runのたびに新しいPKIを作る | D-11 | T-4 | 不可 |
| T-6 | TLSの接続の設定をプールで動かし、`take_conn`を入れる | D-1、D-12、D-13、D-14 | T-3、T-5 | 不可 |
| T-7 | handshakeの押し込みをプールで動かす | D-1、D-12、D-14 | T-6 | 不可 |
| T-8 | rustls-platform-verifierに切り替える | D-10、D-15 | T-7 | 不可 |
| T-9 | Windowsの結果を見直す (条件付き) | D-15 | T-8 | 不可 |
| T-10 | 段を閉じる | すべて | T-8 (当てはまればT-9) | 不可 |

### 各作業で共通の確かめ

- `cargo nextest run` → 0で終わる。`cargo clippy --all-targets -- -D warnings` → 0で終わる。`cargo fmt --check` → 0で終わる
- `grep -rn "docs/plans" crates docs/spec docs/implementation std` → 何も出ない
- テストを流した後の`git status --porcelain crates/eml_vm/tests` → 空 (どの作業も`RunStats`のsnapshotを変えない)

### T-0: 計画を入れる

- 目的: 段の最初のコミット (ロードマップの「進め方」)
- 対象: `docs/plans/2026-11-06-tls-platform-verifier.md`と`docs/plans/2026-11-06-tls-platform-verifier/`
- 作業: そのままコミットする (`Plan tls-platform-verifier`)
- 実装者に任せること: なし
- 完了の条件: `git show --stat HEAD` → `docs/plans/`の下のファイルだけが出る

### T-1: `eml_rt`: `Heap::foreign_take`

- 目的: `Foreign`の箱から値を取り出す (D-1)
- 対象: `crates/eml_rt/src/foreign.rs`、`heap/mod.rs`、`trust.rs`、`heap/tests/trusted.rs`、`docs/spec/object-model.md`
- 作業: `git apply docs/plans/2026-11-06-tls-platform-verifier/patches/01-foreign-take.patch` (コードとテスト。中の日本語のコメントはPLaMoで訳していないので、訳し直す)。そのうえで`docs/spec/object-model.md`を書く。「核の操作」の表で`foreign_unique`の隣に`foreign_take`の行を足す。checked modeの表に行を足す (rc 1でなければ`NotUnique`、`WrongOwner`、両方のmodeで`WrongKind`)。「報告する誤り」の`NotUnique`、`WrongOwner`、`WrongKind`の一覧に`foreign_take`を足す。T3の行の文に、`foreign_take`が値を移した後の箱の破棄処理が何もしないことを足す (`01-pool-jobs.md`の5の作業1)
- 実装者に任せること: 日本語の言い回し
- 条件: いつも
- 完了の条件:
  - `cargo verus verify -p eml_rt` → 最後の行が`verification results:: N verified, 0 errors`
  - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 0で終わり、何も出ない
  - `cargo test -p eml_rt --lib taken_foreign` → `test result: ok. 2 passed`
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::a_taken_foreign_value_is_not_finalized` → 2 passed。`MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows"`でも2 passed
  - `grep -n "foreign_take" crates/eml_rt/src/trust.rs docs/spec/object-model.md` → どちらのファイルからも1行以上出る
  - `crates/eml_rt/CLAUDE.md`のgateの残りが成り立つ
- 落とし穴: `foreign_take`は`foreign_unique`と同じ契約で`&self`と共有の`&BlockPerm`を取る。ghostの模型は`Foreign`のブロックの欄0と1を読まないので、ヒープの状態についての`ensures`は要らない

### T-2: `eml_vm`: 汎用の仕事の記録

- 目的: プールの仕事すべてに1つの記録の種類を使う (D-1。G-1、G-11)
- 対象: `crates/eml_vm/src/cells.rs`、`ffi.rs`、`vm.rs`、古いAPIを名指ししていれば`crates/eml_vm/tests/ffi.rs`、`docs/spec/vm.md`
- 作業: `01-pool-jobs.md`の5の作業2。`OpKind::Job { from, answer }`、`JobSource`、完了の腕は箱を持つ。`OpKind::prefix()`は`Job`に`cannot run a job`を返す。`submit_job(.., from, op)`、`take_job -> JobTaken`、`JobTaken::answer::<T>`。`ffi.rs`は`take_job(..).answer::<u64>()`で`u64`を受け、`ffi_submit`は`JobSource::C(c)`を渡す。食い違いの文言は`an op is taken by another function`にする。`docs/spec/vm.md`の「操作の記録」の種類の一覧にプールの仕事 (仕事の出どころと、プールが返した箱を持つ) を足し、「`blocking`の呼び出し」の完了の箇条を、記録は箱をそのまま持ち、取り出しが`u64`に戻し、違う型は内部の誤り`an op completes with another kind of result`である、と直す
- 実装者に任せること: 非公開の助けの名前
- 条件: いつも
- 完了の条件:
  - `grep -c "OpKind::Ffi\|take_ffi" crates/eml_vm/src/cells.rs` → `0`。`grep -c "downcast" crates/eml_vm/src/cells.rs` → `1` (`JobTaken::answer`の中)
  - `cargo nextest run -p eml_vm --test integration -E 'test(ffi::) | test(ffi_abandon::) | test(bench::)'` → すべて通り、`git status --porcelain crates/eml_vm/tests` → 空
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → すべて通り、`git status --porcelain crates/eml_cli/tests` → 空

### T-3: `eml_vm`: `jobs.rs`とプールの仕事のspec

- 目的: 汎用の`submit`と`take` (D-1。G-3、G-4、G-6、G-10、G-12)
- 対象: `crates/eml_vm/src/jobs.rs` (新しいファイル)、`lib.rs`、`cells.rs`か`jobs.rs`の単体テスト、`docs/spec/vm.md`、`docs/spec/stdlib.md`、`docs/future/ffi.md`
- 作業: `01-pool-jobs.md`の1の2と、5の作業3。`submit`は`impl FnOnce() -> T + Send + 'static`の制約と`catch_unwind`を持つ。`take`は`from`とwakeを確かめ、`Result<T, String>`へdowncastし、`Cancelled`なら`T`を落とす。panicは`Outcome::Failed(io::Error::other(format!("a job panicked: {text}")))`になる。`text`は`&str`か`String`の中身で、それ以外なら`a job panicked`である。単体テスト: `()`を返す仕事を`u64`として取り出すと内部の誤りになる。panicする仕事はpanicの文を持つ`Outcome::Failed`になる。`docs/spec/vm.md`: 「`Async`のタスク」の下、「操作の記録」の後に新しい節「プールの仕事」を置く (手順、`01-pool-jobs.md`の5の値の一生の表、制約、panicの規則、G-13の同じworkerである理由)。「`blocking`の呼び出し」は共通の部分についてこの節を引く。「救済」の6つ目の箇条にプールの仕事の行を足す。`docs/spec/stdlib.md`の「extern の境界」に1段落を足す: stdの行は`submit_*`、`io_wait`、`take_*`の3つ組でRustの処理をプールで動かせる。消費した`Lin`の`Foreign`の引数は仕事へ移り、取り出しが新しい物体として返す。vm.mdの「プールの仕事」を引く。`docs/future/ffi.md`の「長く走る呼び出し」の`Done::Job`の箇条は、vm.mdの「プールの仕事」を引く
- 実装者に任せること: 単体テストを`cells.rs`と`jobs.rs`のどちらに置くか
- 条件: いつも
- 完了の条件:
  - `grep -c "catch_unwind" crates/eml_vm/src/jobs.rs` → `1`。`grep -c "FfiSubmit\|FfiTake" crates/eml_vm/src/jobs.rs` → `0`。`grep -c "FnOnce() -> T + Send + 'static" crates/eml_vm/src/jobs.rs` → `1`
  - `cargo nextest run -p eml_vm --lib -E 'test(jobs::)'` → すべて通る (新しい2つ以上)
  - `grep -c "プールの仕事" docs/spec/vm.md docs/spec/stdlib.md` → どちらも1以上
- 落とし穴: T-6まで、`jobs::submit`にはテストの外の呼び手がない。使われないコードの警告は単体テストで消し、`#[allow]`で消さない

### T-4: 時刻を固定したfixtures、`Name`と`Trust`、VMの側の振り分け

- 目的: 検証器を替える前に、テストの世界を日付とOSから切り離す (D-10、D-11、D-13、D-15)
- 対象: `crates/eml_tls/` (`fixtures/gen.sh`、作り直したPEM、`other-ca.pem`、`src/fixtures.rs`、`src/lib.rs`、`tests/*`)、`crates/eml_vm/src/tls.rs`、`vm.rs`、`crates/eml_extern/src/lib.rs` (`is_async`)、`crates/eml_io/tests/tls.rs`、`crates/eml_cli/tests/tls.rs`、`tests/ui/run-fail/tls/` (`unknown_ca.em`、`server_closes_before_handshake.em`、新しい`bad_ca.em`と`no_roots.em`、`other-ca.pem`) とそのsnapshot、`docs/spec/stdlib.md`、`docs/spec/vm.md`、`docs/implementation/testing.md`
- 作業: `02-verifier.md`の5.4の作業Aを、5.1、5.3、5.5の「Unit A」と「Real-driver test」と合わせて行う。振り分けはD-13のとおりにする (`03-tls-on-pool.md`の5.1。`vm.rs`の2つの`Async`のexternの一覧を含む)。このコミットの`Trust::platform`は、今のコードをそのまま移したもの (webpki-rootsに追加のPEMを足し、現実の時刻で確かめる) である。fixturesは`nix shell nixpkgs#openssl -c crates/eml_tls/fixtures/gen.sh`で作り直す。本物のソケットのテストは`a_real_socket_handshake_with_an_unknown_ca_is_rejected`である (D-15)。文書: `docs/spec/stdlib.md`の「`Std.Tls`」に、テストのdriverの条件で書いた`NoRoots`の行を足す。externの段落には、`client_raw`はrunのdriverの種類を読むので`Async`の命令として動くが、待たないことは変わらない、と書く。`docs/spec/vm.md`の「時計と driver」に、`Machine`がテストのdriverを作ったrunは、渡したCAだけでTLSをrustlsで2026-01-15T00:00:00Zの時刻で確かめ、ほかのrunはOSの検証器を使う、と書く。`docs/implementation/testing.md`の427〜428行 (テストの世界はfixturesの根を使うこと。`other-ca.pem`の写し) と550行 (本物のdriverのテストは拒否のテストであること)
- 実装者に任せること: 日本語の言い回し。`02-verifier.md`の5.5にあるUIテストの頭のコメントは下書きなので、PLaMoで訳し直す
- 条件: いつも
- 完了の条件:
  - `cargo clippy -p eml_vm --all-targets --no-default-features -- -D warnings` → 0で終わる。`cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'` → 2 passed
  - `cargo nextest run -p eml_vm --features tls --lib -E 'test(tls::)'` → 10 passed
  - `cargo nextest run -p eml_core_ir --test integration -E 'test(externs::)'` → 14 passed
  - `cargo nextest run -p eml_tls` → すべて通る
  - `git add`の前の`git status --porcelain crates/eml_cli/tests/snapshots` → 次の4行だけが出る: ` M`の`integration__ui__run_fail@tls__unknown_ca.em.snap`と`integration__ui__run_fail@tls__server_closes_before_handshake.em.snap`、`??`の`integration__ui__run_fail@tls__no_roots.em.snap`と`integration__ui__run_fail@tls__bad_ca.em.snap`
  - `openssl x509 -in crates/eml_tls/fixtures/cert.pem -noout -dates` → `notBefore=Jan  1 00:00:00 2026 GMT`と`notAfter=Feb  1 00:00:00 2026 GMT`
- 落とし穴: 新しいfixturesは現実の時刻では期限切れなので、古い本物のソケットのテストはこのコミットで失敗する。だから消すのはこのコミットである。`bad_ca.em`のsnapshotの位置は、2行の頭のコメントで19:7になる

### T-5: bench: runのたびに新しいPKIを作る

- 目的: fixturesが現実の時刻で期限切れになるので、T-4の後は`bench/scripts.sh`の`https-get`の行が壊れる (D-11)
- 対象: `crates/eml_tls/examples/https_server.rs`、`bench/scripts.sh`、`docs/implementation/benchmarks.md` (162〜172行)
- 作業: `02-verifier.md`の5.3の「Bench」。`https_server`は`CERT_PEM_PATH KEY_PEM_PATH MIB`を取る。行は`/usr/bin/openssl`で2日有効なCAとleafを作り、両方に`subjectKeyIdentifier=hash`を、leafに`authorityKeyIdentifier=keyid`を付ける (AKIがないとPython 3.14が鎖を拒む)。benchmarks.mdには、この行がrunのたびにPKIを作ると書く
- 実装者に任せること: shellの細部
- 条件: いつも
- 完了の条件: macOSで`bench/scripts.sh`が`https-get`の行を出し、標準エラーに`rejected`も`CERTIFICATE_VERIFY_FAILED`も出ない

### T-6: TLSの接続の設定をプールで動かし、`take_conn`を入れる

- 目的: Linuxで根のファイルを読む`Conn`の作成を、プールで動かす (D-12、D-14)
- 対象: `crates/eml_extern/src/lib.rs`、`crates/eml_vm/src/tls.rs`、`vm.rs`、`std/Tls.em`、`crates/eml_core_ir/tests/externs.rs`、`tests/ui/run-fail/tls/cancelled_setup.em`、`tests/ui/run/tls/timeout_connect.em`とそのsnapshot、`docs/spec/stdlib.md`、`docs/spec/core-ir.md` (233行の表)
- 作業: `03-tls-on-pool.md`の5.2 (S1a)、5.5のS1aのテスト、5.6のS1aのspecの文。`Outcome::Failed(e)`は`cannot run TLS: {e}`を投げる。`client_raw`と`TlsClientRaw`への言及をすべて消す。T-4が`is_async`のdoc commentとstdlib.mdに足したものも含む
- 実装者に任せること: 日本語の言い回し
- 条件: いつも
- 完了の条件:
  - `cargo clippy -p eml_vm --all-targets --no-default-features -- -D warnings` → 0で終わる。`cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'` → 2 passed
  - `cargo nextest run -p eml_core_ir --test integration -E 'test(externs::)'` → 14 passed
  - `cargo nextest run -p eml_vm --features tls --lib -E 'test(tls::)'` → 10 passed
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(tls::)'` → すべて通る。`git status --porcelain crates/eml_cli/tests/snapshots` → `??`の`integration__ui__run_fail@tls__cancelled_setup.em.snap`と`integration__ui__run@tls__timeout_connect.em.snap`の2行だけ
  - `grep -rn "client_raw\|TlsClientRaw" crates std docs/spec docs/implementation` → 何も出ない
  - `cancelled_setup.em`のsnapshotが`task was cancelled`で終わる。`timeout_connect.em`が`0: timed out`と`1: connected`を出す

### T-7: handshakeの押し込みをプールで動かす

- 目的: 検証しうる押し込みをプールで動かす (D-12)
- 対象: `crates/eml_extern/src/lib.rs`、`crates/eml_vm/src/tls.rs`、`vm.rs`、`std/Tls.em`、`crates/eml_core_ir/tests/externs.rs`、`docs/spec/stdlib.md`、`docs/spec/core-ir.md`
- 作業: `03-tls-on-pool.md`の5.3 (S1b)、5.5のS1bの単体テスト、5.6のS1bのspecの文
- 実装者に任せること: 日本語の言い回し
- 条件: いつも
- 完了の条件:
  - 共通の確かめ、T-6の`tls_off::`とfeatureなしのclippy、`externs::` → 14 passed
  - `cargo nextest run -p eml_vm --features tls --lib -E 'test(tls::)'` → 11 passed
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → すべて通り、`git status --porcelain crates/eml_cli/tests` → 空
  - この行に当たるD-1の確かめがすべて成り立つ
    - G-1: `grep -n "jobs::submit" crates/eml_vm/src/vm.rs` → `TlsSubmitClient`と`TlsSubmitPush`の腕の2行
    - G-2: `awk '/Extern::TlsSubmitPush =>/,/^        }/' crates/eml_vm/src/vm.rs | grep -n "foreign_take::<\|decref(\|jobs::submit"` → この3つがこの順に、増える行番号で出る
    - G-4: `for f in $(grep -rl "jobs::submit" crates/eml_vm/src); do perl -0ne 'while (/jobs::submit(?:::<[^>]*>)?\((.*?)\)\?;/sg) { my $c = $1; $c =~ s/^.*?move \|\|//s; print "$c\n" }' "$f"; done | grep -c "heap\|Word"` → `0`。この繰り返しが出すクロージャの数は、`jobs::submit`の呼び出しの数 (`jobs.rs`の定義を除く) と等しい。クロージャの本体に`)?;`を含めない
    - G-5: `grep -n "submit_client\|submit_push" std/Tls.em | grep -v "^[0-9]*:extern "` → ちょうど2行で、どちらも`Async.io_wait (submit_`を含み、それぞれの次の行が`  take_conn op w`である。`grep -c "io_wait_until (submit_" std/Tls.em` → `0`
    - G-6: `grep -n -A6 "Outcome::Cancelled" crates/eml_vm/src/vm.rs` → `TlsTakeConn`の腕に`IO_CANCELLED`と`"task was cancelled"`が出る
    - G-7: `grep -n "^pub extern submit_\|^pub extern take_conn" std/Tls.em` → 何も出ない
    - G-9: 作業ツリーの写しで`Extern::TlsSubmitPush`の腕の本体を`return Err(VmFault::Internal("probe"))`に替えると、`cargo nextest run -p eml_cli --test integration -E 'test(ui::run)'`が失敗し、`tests/ui/run/tls/echo.em`の名前が出る。替えない作業ツリーでは同じコマンドが通る
  - コミットのメッセージに、`tls/`はseedで流さないので、TLSの仕事の行はseedの流しで確かめていないこと (G-4) を書く
- 落とし穴: T-6だけでも、公開の入口をgrepするG-9の確かめは通ってしまう。押し込みの行にchecked modeで届くことを示すのは、書き換えによる確かめのほうである

### T-8: rustls-platform-verifierに切り替える

- 目的: OSの検証器を使い、webpki-rootsを外す (D-10、D-15)
- 対象: `Cargo.toml`、`Cargo.lock`、`crates/eml_tls/Cargo.toml`、`src/lib.rs`、`tests/platform.rs` (新しいファイル)、`tests/main.rs`、`about.toml`、`THIRD-PARTY-LICENSES.md`、`docs/spec/stdlib.md`、`docs/implementation/testing.md`
- 作業: `02-verifier.md`の5.4の作業Bと、5.5の「Unit B」。`Trust::platform*`は、`Opaque`で包んだ`rustls_platform_verifier::Verifier::new_with_extra_roots(roots, provider())`にし、`platform_at`を足す。webpki-rootsを外す。`platform.rs`を足す。`about.toml`から`CDLA-Permissive-2.0`を外し、`nix develop -c nix/third-party-licenses.sh`を流す。`docs/spec/stdlib.md`の「`Std.Tls`」は`02-verifier.md`の5.6のとおりに書く: OSごとの検証器。CAはOSの根に加えて信頼すること。検証がネットワークに出ることがあること。`NoRoots`の行にLinuxの条件とCAを渡す勧めを足すこと。検証の中のOSの側の失敗も拒否として報告すること。`Std.Http`の`ca`は空ならOSの根だけであること。`docs/implementation/testing.md`に1段落を足す: OSの検証器に届くテスト (`platform::`、`eml_cli`の`tls::`、nixのsandboxの代わりのLinuxの子のテスト)、fixturesにAIAもCDPもないこと、OSの根に頼るテストがないこと
- 実装者に任せること: 日本語の言い回し
- 条件: いつも
- 完了の条件:
  - `grep -c 'name = "webpki-roots"' Cargo.lock` → `0`。`grep -c 'name = "rustls-platform-verifier"' Cargo.lock` → `1`
  - `cargo nextest run -p eml_tls` → すべて通る (macOSでは`platform::`の3つが通り、Linuxだけの2つはコンパイルされない)
  - `nix develop -c nix/third-party-licenses.sh --check` → 0で終わる
  - `.github/workflows/ci.yml`の`feature resolution of tls`の4つのコマンドが、前と同じ結果になる (照らすのは`eml_tls|aws-lc|ring v`だけである)
  - `git status --porcelain crates/eml_cli/tests/snapshots` → 空
  - branchをpushした後、CIの実行が4つのOSすべてで通り、windows-2025では`eml_tls::integration platform::`と`eml_cli::integration tls::`のnextestの行がどれも1.000秒未満である (`gh run view --log`でWindowsのテストのjobを見る)
- 落とし穴: nixのsandboxではシステムの根が空である。CIではLinuxの子のテストがこの条件を確かめる。rustls-platform-verifierの`dbg`と`ffi-testing`のfeatureは使わない

### T-9: Windowsの結果を見直す (条件付き)

- 目的: D-15の、だめだったときの扱い
- 条件: T-8のWindowsの確かめが成り立たないとき (`platform::`か`eml_cli`の`tls::`のテストが、コードに原因がないのに1秒以上かかるか失敗する) だけ行う。成り立てばT-9は飛ばし、T-10のコミットのメッセージにそう書く
- 作業: 止めて、種類1の変更の承認を利用者に求める。変更は、当たったテストに`#[cfg(not(windows))]`を付けることと、testing.mdの「OS ごとのテスト」に項目を足すことである。`#[ignore]`は使わない (無視したテストはCIで流れない)
- 実装者に任せること: なし
- 完了の条件: 利用者が承認し、CIの実行が4つのOSすべてで通る

### T-10: 段を閉じる

- 目的: ロードマップの「進め方」の閉じる手順 (`Close tls-platform-verifier`)
- 作業: 次の順に行う
  1. `docs/spec/`に、段の名前と「今は〜、後で〜」の予告が残っていないことを確かめる
  2. 計測する。`std/`、`crates/eml_vm`、`crates/eml_rt`が変わったので対象である。まず`bench/scripts.sh`を流し、範囲を外れた行があれば別のコミット`Resize bench rows`で大きさを変える。次に`bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh`をコードの最後のコミットの作業ツリーで流し、[計測の基準](../implementation/benchmarks.md)の「今の数値」の3つの表を入れ替える。macOSがなければ、直後の別のコミット`Measure tls-platform-verifier`にする
  3. `docs/implementation/status.md`の42行と150行: 根はOSの根と渡したCAであること。接続の設定とhandshakeはプールで動くこと
  4. `docs/implementation/architecture.md`: 50行、81行、96行。「`eml_tls`の内部」(`Name`、`Trust`、`Conn::client`。プロセス全体の状態を持たないこと。Linuxは作るたびに根のファイルを読み直すこと。接続ごとの`ClientConfig`と、接続をまたぐresumptionがないこと。追加の根の確かめと誤りの対応。`Opaque`とその診断の損。fixturesの期間、`NOW`、`other-ca.pem`、fixturesが現実の時刻で期限切れであること。`https_server`の引数)。`eml_vm`の部分 (322行: `client_conn`、`jobs.rs`を通る設定とhandshakeの仕事、`jobs.rs`そのもの)。外部crateの表 (webpki-rootsの代わりに`rustls-platform-verifier | 0.7 | eml_tls の証明書の検証 …`)。webpki-rootsとCDLAを除いたライセンスの段落
  5. `docs/future/roadmap.md`: この段の項目を消す。415行 (musl): 静的な配布物には、根のファイルか`SSL_CERT_FILE`、またはCAを渡すプログラムが要る、と直す。「その後の項目」にstdの`extern`の行の`blocking`の属性を足す (後に回した理由: Rustの仕事の行の族は1つしかなく、手で書く3つ組は2つの行と3行のemlで済む。見直す条件: プラグインやGUIのような、仕事の行の3つ目の族が現れたとき)
  6. `docs/overview.md`の56行 (TLSの行) と、`docs/README.md`の「文書の地図」の、内容が変わった所だけを直す
  7. `CLAUDE.md`と`README.md`のcrateの行: `eml_tls`を`(aws-lc-rs provider, rustls-platform-verifier)`にする
  8. `docs/`に残っていない決めがないことを確かめてから、`docs/plans/2026-11-06-tls-platform-verifier.md`とそのディレクトリを消す
  9. コミットのメッセージに、文書で何をどこへ移したか、テストの変更の種類と理由 (7の表)、計測の目立つ差、大きさを変えた行、後に回した項目とその決めた日を書く
- 実装者に任せること: 日本語の言い回し
- 条件: いつも
- 完了の条件:
  - `grep -rn 'webpki-roots\|webpki-root' docs CLAUDE.md README.md crates std bench about.toml` → 何も出ない
  - `grep -rn 'tls-platform-verifier' docs CLAUDE.md` → 何も出ない
  - `ls docs/plans` → ディレクトリがないか、空
  - 共通の確かめ

## 8. 実行の順

T-0 → T-1 → T-2 → T-3 → T-4 → T-5 → T-6 → T-7 → T-8 → (T-9) → T-10

- T-1〜T-3とT-4〜T-5は独立していて並べて準備できるが、コミットはこの順に入れる
- T-6にはT-3 (`jobs.rs`) とT-4 (振り分け) が要る。T-5をT-6より先に入れ、benchの行が壊れているコミットをT-4の1つだけにする
- T-8をT-7より先に入れてはならない。T-8からはhandshakeの押し込みがOSの検証器でブロックしうるが、それをプールへ移すのはT-7である

## 9. 検証

- 基準の状態 (基準のコミット、T-0の前): `cargo nextest run` → 0で終わる。`cargo clippy --all-targets -- -D warnings` → 0で終わる
- 全体の検証 (T-10の後): 共通の確かめ。`cargo verus verify -p eml_rt` → 0 errors。`nix develop -c nix/third-party-licenses.sh --check` → 0で終わる。PRのCIが4つのOSすべてで通る
- 利用者が手で確かめること
  - aarch64-darwin: `nix build`が成功する (check phaseがmacOSの検証器で`platform::`を流す)
  - nixのsandboxを有効にしたx86_64-linuxがあれば: `nix build`が成功する (まだ確かめていない。CIではLinuxの子のテストが同じ条件を確かめる)
  - どれかのOS: `Tls.connect "example.com" 443`でGETする小さなプログラムを`eml run`で流し、システムの根で成功する

## 10. 危険と未解決の項目

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | システムの証明書ストアで最初に鎖を組むときの、Windowsの根の自動更新 | T-8のWindowsのCIの時間 | T-8のPRのCIの実行で分かる | 受け入れる。だめならT-9 (利用者の承認が要る) |
| Q-2 | 押し込みの仕事の最中に取り消す、決定的なテストがない (仮想の時計の6つの組み立てで、どれも`Net`の読みで取り消された) | T-7 | この段では扱わない | `take_conn`の共通の取り消しの道は`cancelled_setup.em`が、`T`を落とすことは`jobs::take`の単体テストが確かめる |
| Q-3 | 1秒の仕事の予算より長い検証の途中の割り込みか`exit`は、`Machine`をpoisonedにする (REPLはセッションを終える) | 遅いネットワークで利用者に見える | 受け入れた。T-6でspecに書く | 決めたとおり |
| Q-4 | sandboxを有効にしたLinuxの`nix build`を流していない | パッケージ | 9の利用者の手の確かめ | Linuxの子のテストに頼る |
| Q-5 | macOSとWindowsのEd25519のサーバーの証明書 (rustls-platform-verifier #195) | 拒まれる | 扱わない | 拒否として見える |

## 付録: 査読での主な変更

- テストの検証器を選ぶ`RunConfig.tls_verify`の欄 → テストのためだけの欄として退けた。driverの種類で検証器を選ぶ (D-11)
- プロセス全体の検証器のcache → なくした (D-10)
- すべての`push_tls`をプールで動かす → handshakeの押し込みだけにした。ネットワークは内部の誤りに届かない (D-12)
- 本物のソケットのTLSのテスト → 拒否のテストにし、T-4で消して置き換える (D-15)
- プールの仕事のpanicでプロセスが終わる → `jobs::submit`が捕まえる (D-1)
- G-4、G-5、G-9の確かめが書いたとおりでは通りえなかった → 利用者が採った直した文にした (D-1)
- 検証器の側の設計とTLSの側のインターフェースが食い違った → T-4が`03-tls-on-pool.md`の5.1のインターフェースをそのまま作る (D-13)
