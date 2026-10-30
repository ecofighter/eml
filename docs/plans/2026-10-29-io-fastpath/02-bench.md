# 付録2: `bench/net` の物差しと、前と後を比べる規則と手順

この付録は、本体の T-1 (`eml run --stats`)、T-2 (`bench/net`)、T-2b (1点目)、T-9 (`busy.em` と `POLL_EVERY` の比べ)、T-10 (2点目) の細部と、後の段 io-backends の3点目の取り方を持つ。本体と食い違うときは本体に従う。

- 8 節のファイルは、そのまま `bench/net/` と `bench/net.sh` に置ける形で写してある。T-2 はこの形で入れ、T-8 が 8.3 節の差分を当て、T-9 が 8.4 節の `busy.em` と `busy-self` の行を足す。
- 7 節の数字は基点 (41b4f3d) の release の eml で測った今の値である。1点目は T-2b で測り直し、7 節の値は比べの手がかりにだけ使う。
- 機械は Linux 6.18 の container (Xeon 2.1 GHz、4 vCPU、16 GiB) で、`strace`、`valgrind`、`taskset`、`python3` がある。`perf` と eBPF はない。

## 1. `eml run --stats` (T-1)

- 本物の driver の `RunStats` を読む口を、環境変数でなく `run` の flag にする。`--help` に出て、ほかの run の option (`--debug-heap`、`--sched`) と並び、CLI のテストで確かめられるからである。
- 意味: 検査モードで走らせる (`--debug-heap` と同じ)。run が `Finished` で終わったら (`exit` の終わりも含む)、標準エラーに1行を出す。実行時エラーで終わったときは出さない。
- 形: `stats: <欄>=<値> <欄>=<値> ... preemptions=<値>`。欄は `RunStats` の宣言の順で、名前は Rust の欄の名前そのままである。最後に `Finished::preemptions` を置く。
- 実装: `impl RunStats { pub fn fields(&self) -> [(&'static str, u64); 23] }` が欄の名前と値の組を宣言の順で返し、CLI はそれを並べるだけにする。欄を足すと表の行が1つ増え、出力と `report.py` の列に出る。
- `main.rs` の `Run` に `#[arg(long)] stats: bool` を足し、`debug_heap || stats` で検査モードにする。`Ok(finished)` のとき `eprintln!("stats: {} preemptions={}", 並び, finished.preemptions)` を出す。REPL には足さない。
- テスト:
  - `crates/eml_cli/tests/cli.rs` の `run_with_stats_prints_one_stats_line`: 標準エラーの1行が `stats: ` で始まり、`parks=`、`completions=`、`preemptions=` を含む。標準出力は `--stats` なしの run と同じ
  - `eml_vm` の単体テスト: `fields()` の名前の並びが `format!("{:?}", RunStats::default())` の欄の名前の並びと同じ (欄を足して表を忘れると落ちる)
- `--stats` の run は検査モードなので、その run の時間は物差しに使わない。時間は `--stats` なしの速いモードの run から取り、`RunStats` は `--stats` の別の1回の run から取る。`bench.py` はそう流す。

## 2. 件 (`cases.tsv`)

eml の側は `Std.Net` で書く。相手は同じ run の中の eml のタスク (`-self`) か `peer.py` (`-py`) である。`-py` の run では eml のプロセスだけを測る。名前が `py:` で始まる件は Python 同士の床で、時間だけを測る。

| 名前 | 時間の引数 / 小さい引数 | 何をするか | 単位 |
|---|---|---|---|
| base | - | Net と Async を読み込み、listen して閉じるだけ。コンパイルと起動の床 | run |
| compute-fib | `../fib.em` | I/O のない単一タスクの run。監視のスレッドを run の始めに起こす費用を見る (T-2 で足す) | run |
| echo-self-fresh | `echo.em 20000 16 fresh` / 2000 | 1本の接続で 16 バイトの往復を N 回する。書くたびに `Bytes.make`、読むたびに `Bytes.empty ()`。相手のタスクも `Bytes.empty ()` で読み、読んだものを返す。両端とも `set_nodelay True` | 往復 |
| echo-self-reuse | `echo.em 20000 16 reuse` / 2000 | 両端とも1つの buffer を `Bytes.truncate 0` で空にして読みに渡し直す | 往復 |
| echo-py-fresh / echo-py-reuse | 上に `{port}` を足す / 2000 | eml のクライアントと Python の echo サーバー | 往復 |
| bulk-self | `bulk.em 1024 64 fresh fresh` / 100 MiB | 64 KiB を書くたびに `Bytes.make`。読み手は毎回 `Bytes.empty ()` で EOF まで読む | MiB |
| bulk-shared | `... shared fresh` | 同じ1つの `Bytes` を毎回書く | MiB |
| bulk-reuse | `... fresh reuse` | 読み手が buffer を `truncate 0` で使い回す | MiB |
| bulk-1k | `bulk.em 64 1 fresh fresh` / 16 MiB | 1 KiB ずつ書く | MiB |
| bulk-send-py | `bulk.em 1024 64 fresh fresh {port}` と `sink-server` | eml は送るだけ | MiB |
| bulk-recv-py | `bulk.em 1024 64 fresh fresh -{port}` と `source-server` | eml は読むだけ。Python の送りが先行するので、ほとんどの読みでデータがもう来ている | MiB |
| bulk-accum | `bulk.em 16 64 fresh accum` / 16 MiB | 1つの buffer に足し続ける。写しの伸び方を見る | MiB |
| many-self-250 / 1000 / 2000 | `many.em C` / 同じ | クライアントは C 本を1本ずつ順に connect する。張り終えたら接続ごとのタスクで 16 バイトを書いて返事を読み、閉じる。サーバーは受けるたびにタスクを始め、読んで返し、EOF を読んで閉じる。張る間はサーバーの側に、張った本数だけの読みが保留になる | 接続 |
| many-py-1000 | `many.em 1000 {port}` と `many-server 1000` | eml はクライアントだけ | 接続 |
| conn-self-1 | `conn.em 10000 1` / 1000 | 1本ずつ順に connect して EOF を読んで閉じる。サーバーは accept してすぐ閉じる | 接続 |
| conn-self-32 | `conn.em 10240 32` / 1024 | 32 個のタスクが同時に同じことをする | 接続 |
| conn-py | `conn.em 10000 1 {port}` と `conn-server 10000` | eml はクライアントだけ | 接続 |
| busy-self | `busy.em 2000` / 200 | 同じ run の中の 16 バイトの echo と、計算だけのタスク1つ。集める地点が OS を見る頻度の比べに使う (T-9 で足す。8.4 節) | 往復 |
| py:echo、py:bulk、py:conn | 20000 往復、1024 MiB、10000 接続 | Python 同士の床 | |

- 「buffer を使い回す echo」は `echo-*-reuse`、「`Bytes.empty ()` を毎回渡す echo」は `echo-*-fresh` である。
- many は backlog を超えない形にしてある。クライアントが1本ずつ connect するので、4000 本まで `ListenOverflows` は増えなかった。同時に張る形では 1500 本と 2000 本で `ListenOverflows` と `TCPSynRetrans` が増え、SYN の再送が数字に混ざった。件は 250、1000、2000 本にする。
- conn は EOF を待つ形を残す。待たないと backlog が埋まって SYN の再送 (1 秒) を待ち、物差しにならない。そのため1接続の値には close と EOF の読みが入る。
- `busy-self` は1点目の後に入る。1点目と2点目の比べからは外し、2点目と3点目で比べる。

## 3. 数える値と測り方

| 値 | 測り方 | 回数 |
|---|---|---|
| 主指標: eml のプロセスの CPU (user+sys) を、床の中央値で引いて単位で割った µs | `run.py` の rusage (`os.wait4`) | 時間の run を 11 回 (前と後を交互に) |
| 実時間 (単位あたり)、sys、最大 RSS、`minflt` (単位あたり)、自発と非自発の切り替え | 同上 | 同上 |
| syscall の数。main のスレッドと、ほかのスレッド (`eml-monitor` など `PR_SET_NAME` の名前) に分け、名前ごと、失敗の errno ごとに数える | `strace -f -qq -o` の生の出力を `run.py` が数える。床の run を引いて単位で割る | 3 回。範囲を出す |
| `RunStats` のすべての欄 (`parks`、`completions`、`allocations`、`rc_*`、`string_bytes_copied`、`tasks`、`preemptions` など) | `eml run --stats` | 1 回 |
| 補助: 命令の内訳、ヒープの山 | callgrind、massif。判断の分かれる件にだけ使う | |

- `report.py` が出す `RunStats` の欄は、床の行にある名前だけである。
- 速い道 (終わった操作を待たないこと) は、`RunStats` の欄でなく、`parks` と `completions` の意味 (本体 D-3) と、テストの driver の数 (`TestDriver::immediate()`) で見る。`RunStats` にその場で終わった数の欄は足さない。
- syscall の列の `epoll_wait` は、compio の上では 0 になる (compio は `epoll_pwait` を使う)。自前の backend が `epoll_wait` を使えば、そこに出る。

## 4. 比べる規則

- 主指標は eml のプロセスの CPU (user+sys) を床で引き、単位で割った値である。各ラベル 11 回の中央値を使い、前と後は件ごとに交互に流す。`-py` の件の実時間は Python の相手が律速するので、eml の違いは CPU にしか出ない。
- 判定: 後 ÷ 前の比が 0.90 以下か 1.10 以上で、かつ生の CPU の四分位の範囲 (25%〜75%) が前と後で重ならないときだけ「差」とする。そうでなければ「差なし (この機械では見分けられない)」と書く。
  - 根拠: 同じ実行の道の2つの eml を比べた A/A の run では、18 件のどれもこの規則で「差」にならなかった。比だけなら 0.93 から 1.24 まで開いた。
  - 最小値は採らない。A/A で最小値の比は 0.88 から 1.55 と、中央値の比より広かった。
  - CPU は固定しない。`taskset` で固定しても揺れは狭まらなかった (四分位の幅が狭まったのは echo の2件だけで、conn-self と many-self-1000 は広がった)。
- `minflt` の単位あたりの値が前と後で 1 以上違う件は「確保の揺れ」と印を付け、時間の判定から外す。1往復ごとに確保して手放す buffer が glibc のヒープの頂きを毎回縮めて伸ばすと、echo-self-fresh の CPU は1往復あたり約 8 µs 増える。どちらになるかはバイナリの配置で変わり、物差しでは決まらない。その件の確保の違いは `RunStats` の `allocations` と syscall の `brk` で見る。
- 実時間は副の指標として並べる。bulk の帯域と many-self の伸び方 (2000 本 ÷ 250 本の1接続あたり CPU の比。基点で 2.96) は実時間でも書く。
- syscall は 3 回流して範囲を書く。`sendto`、`recvfrom`、`accept4`、`connect` と errno ごとの数は、3 回で同じか 1% 以内だった。poll に付く数 (`epoll_pwait`、`timerfd_settime`、`read(eventfd)`、`epoll_ctl`) は、bulk-recv-py で 3.6%、many-self-1000 で 8% まで揺れた。syscall の目標の確かめは、3 回のどれもが目標以下であることとする。
- syscall は main のスレッドで比べる。driver が別のスレッドを持つようになったら、その名前の列が足される。

## 5. 置き場所と時期

- `bench/net/` に `echo.em`、`bulk.em`、`many.em`、`conn.em`、`base.em`、`cases.tsv`、`peer.py`、`run.py`、`bench.py`、`report.py` を置き、入口を `bench/net.sh` にする (T-2)。T-9 が `busy.em` を足す。`args.inc`、`netstat.py`、`var.py`、計測の生の出力は入れない。
- `bench/run.sh` は `bench/*.em` だけを流し、`crates/eml_vm/tests/bench.rs` の `every_program_in_bench_has_a_test` は `bench/` の直下の `.em` だけを数える。下のディレクトリならどちらにもかからない。プログラムは1ファイルで動くので、import を解く道具は要らない。
- `bench/net.sh` はテストではないので、testing.md の 1 秒の規則にかからない。CI では流さない。`cargo nextest run` に I/O の時間の assert は入れない。
- 道具: Linux では `python3` と `strace` を使う (`taskset` は `--pin` のときだけ)。build は fenix の cargo (1.99.1。`rust-toolchain.toml` と同じ版) を使う。git の worktree の中で `nix develop` が `getting Git object ... object not found` で止まったら、`nix develop "path:$PWD"` か、fenix の cargo を直に使う。nix の devShell が `strace` を持つかは確かめていない。
- macOS: 時間と RSS と `RunStats` を取り、syscall は取らない (`run.py` が `--strace` と `--pin` を断る)。`ru_maxrss` はバイトなので KiB に直す。この分岐は macOS で流していない。`kern.ipc.somaxconn` の既定は 128 なので、eml の backlog 1024 も Python の `listen(4096)` も 128 に切られる。conn-self-32 と many-self は、最初に `netstat -s` の listen の欄で SYN の再送が混ざらないかを確かめる。
- Windows: io-fastpath と io-backends の間、本物の driver はソケットを扱わないので流せない。net-windows の後で時間だけを流す。
- benchmarks.md に「ネットワークの計測」の節を置く (T-2 は節と規則だけ。表は T-10 から)。3点 (compio の前 (io-fastpath の始め)、io-fastpath の後、io-backends の後) を同じ機械で取り、`report.py` の「比べ」の表と、syscall の目標の対応の表 (7.2 節の形) を置く。機械と kernel を書き、列はコミットで名付ける。後の段 (`io-uring` など) も同じ表に点を足す。

## 6. 前と後を比べる手順

1. 同じ機械を使う。1点目と同じ形の Linux の container で、ほかに負荷のない状態にする。benchmarks.md の「ネットワークの計測」に書いた機械と kernel を確かめる。
2. 前と後のコミットを別々の worktree に取り出し、それぞれ別の `CARGO_TARGET_DIR` で `timeout 1200 cargo build --release -p eml_cli` して、`eml-before` と `eml-after` に写す。前の eml も `--stats` を持つコミットにする (1点目は T-2 のコミット)。
3. `python3 -I bench/net/bench.py --runs 11 --strace-runs 3 --stats before=./eml-before after=./eml-after > results.jsonl` を流す。全件を2ラベルで流すと、時間の run が約 5 分、strace と `RunStats` が約 5.5 分かかった。`--only REGEX` で件を絞れる。前の eml が今のプログラムを型検査できなければ、前のコミットの bench/net を `git archive <前のコミット> bench/net | tar -x -C <置き場所>` で取り出し、`before=./eml-before@<置き場所>/bench/net` と書く。そのラベルだけが取り出したプログラムを流し、時間の run は今までどおり交互になる。
4. `python3 -I bench/net/report.py results.jsonl > summary.md` の「比べ」の表で、4 節の判定を読む。`minflt` の列が 1 以上違う件は「確保の揺れ」として時間の判定から外す。
5. syscall の表で、7.2 節の各項目を対応の件と列で確かめる。3 回のどれもが目標以下であることを見る。
6. `RunStats` の表で、`parks`、`completions`、`allocations` を往復、MiB、接続あたりで並べる。`parks` は2列で同じになる (プログラムで決まる)。`allocations` は、buffer を使い回す件で往復あたり 2 以上減ることを見る。`completions` は、受け付けで終わる操作も数えるので、io-fastpath の後も操作の数と同じである。
7. many-self の1接続あたりの CPU の伸び (2000 本 ÷ 250 本) と RSS を並べる。io-backends の driver の表の形 (`HashMap` のままにするか) は、この比で決める。
8. benchmarks.md の節に表を足し、列をコミットで名付ける。

3点の取り方:

| 点 | いつ | 前と後 | 入れる所 |
|---|---|---|---|
| 1点目 (compio の前) | T-2b | T-2 のコミットの eml だけを1ラベルで流す | T-2b が計画書に、T-10 が benchmarks.md に |
| 2点目 (io-fastpath の後) | T-10 | `before=<T-2 の eml>`、`after=<T-9 の後の eml>` | T-10 が benchmarks.md に |
| 3点目 (io-backends の後) | io-backends の `Close io-backends` | `fastpath=<Close io-fastpath の eml>`、`backends=<io-backends の差し替えの後の eml>` | 同じコミットが benchmarks.md に |

- `busy-self` と `compute-fib` の扱い: `compute-fib` は1点目から測る。`busy-self` は T-9 で入るので1点目に値がなく、1点目と2点目の比べから外す。io-backends の3点目では `busy-self` も測る。
- 1点目と同じ形の Linux の機械がなければ、T-10 と `Close io-backends` は段を閉じずに止めて利用者に報告する。macOS でしか流せない「今の数値」の3つの表 (`bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh`) だけを、閉じた直後の別のコミット `Measure <段>` にする。

## 7. 基点の数字 (41b4f3d の release の eml)

ラベル A は `--stats` を足した eml、B は基点そのままの eml である。違いは CLI の `main.rs` だけで、実行の道は同じなので、A と B の比べがそのまま A/A の確かめになった。

### 7.1 時間 (B。11 回の中央値)

| 件 | CPU s 中央値 (四分位) | 単位あたり CPU µs | 単位あたり実時間 µs | 単位あたり sys µs | 最大 RSS MiB | minflt/単位 |
|---|---|---|---|---|---|---|
| base | 0.032 (0.029-0.036) | - | - | - | 12.3 | 1245 (run あたり) |
| echo-self-fresh | 0.917 (0.779-0.959) | 44.27 /往復 | 44.47 | 23.73 | 12.5 | 3.00 |
| echo-self-reuse | 0.520 (0.497-0.554) | 24.40 | 25.32 | 10.59 | 12.5 | 0 |
| echo-py-fresh | 0.777 (0.730-0.790) | 37.25 | 41.29 | 19.21 | 12.4 | 0 |
| echo-py-reuse | 0.758 (0.724-0.772) | 36.32 | 39.86 | 20.18 | 12.4 | 0 |
| py:echo | 0.441 (0.433-0.456) | 22.05 | 43.03 | 17.57 | 12.3 | 0.07 |
| bulk-self | 0.372 (0.352-0.385) | 332 /MiB | 334 (約 3.0 GiB/s) | 177 | 12.6 | 0.09 |
| bulk-shared | 0.358 (0.344-0.380) | 319 | 323 | 171 | 12.5 | 0.10 |
| bulk-reuse | 0.356 (0.339-0.376) | 317 | 323 | 169 | 12.6 | 0.09 |
| bulk-1k | 0.841 (0.795-0.897) | 12 640 /MiB | 12 895 | 5 562 | 12.5 | 0.86 |
| bulk-send-py | 0.591 (0.571-0.621) | 546 | 548 | 404 | 12.5 | 0.06 |
| bulk-recv-py | 0.518 (0.485-0.544) | 475 | 668 | 262 | 12.5 | 0.07 |
| py:bulk | 0.494 (0.463-0.508) | 483 | 501 | 425 | 12.3 | 1.31 |
| bulk-accum | 1.140 (1.097-1.208) | 69 253 /MiB | 68 976 | 61 235 | 44.3 | 33 198 |
| many-self-250 | 0.073 (0.065-0.084) | 165 /接続 | 277 | 69 | 18.8 | 6.78 |
| many-self-1000 | 0.357 (0.300-0.385) | 325 | 381 | 99 | 53.1 | 10.55 |
| many-self-2000 | 1.007 (0.901-1.113) | 488 | 546 | 131 | 222.0 | 27.09 |
| many-py-1000 | 0.112 (0.097-0.123) | 81 | 122 | 51 | 25.3 | 3.41 |
| conn-self-1 | 0.405 (0.356-0.504) | 37.33 /接続 | 39.06 | 24.87 | 12.4 | 0 |
| conn-self-32 | 0.505 (0.461-0.549) | 46.25 | 47.53 | 23.77 | 12.5 | 0 |
| conn-py | 0.584 (0.569-0.605) | 55.21 | 58.42 | 36.76 | 12.3 | 0 |
| py:conn | 0.527 (0.493-0.547) | 52.68 | 80.72 | 35.07 | 12.3 | 0.13 |

表の手順の外で1回ずつ流した分: many-self の 250、500、1000、2000、4000 本は実時間 0.114、0.193、0.355、0.918、4.475 s、user 0.06、0.119、0.192、0.628、3.642 s、最大 RSS 18.8、25.3、52.4、220.7、503.3 MiB だった。bulk-accum の 4、16、64 MiB は実時間 0.111、1.162、22.807 s だった。

### 7.2 syscall の目標と基点の数 (strace -f、main のスレッド、床を引いて単位あたり)

| 項目 | 目標 (本体 D-9 の G-23) | 基点 | 測る件と列 |
|---|---|---|---|
| 書けるソケットへの `write_bytes` | 1 | 1 (`sendto`) | bulk-send-py の `sendto` と main の計 |
| データのある `read_chunk` (空きあり) | 1 | 9 (ADD、poll の 5 回、DEL、`recvfrom`。`Bytes.empty ()` でも空きのある buffer でも同じ) | bulk-recv-py、echo-*-reuse。`recvfrom` 1回あたりの main の計 |
| 待つ `read_chunk` | 3 | 8 (echo-py)、13 (echo-self) | echo-py-*。`recvfrom` と `recvfrom!EAGAIN` と `epoll_wait`/`epoll_pwait` |
| backlog のある `accept` | `accept4` 1 と受けたソケットの登録 1 | `accept4` は 1。ほかに epoll の分が付く | conn-self-32 の `accept4` (EAGAIN 0) |
| 空きのループの1回の待ち | 1 | 5 | どの件も `epoll_pwait` 1回あたりの `timerfd_settime`、`epoll_ctl`、`read(eventfd)` |
| 操作ごとの `epoll_ctl` | 0 | 待つ読み1回あたり 4 | `epoll_ctl` ÷ 操作の数 (echo-py なら往復1回が操作2つ) |
| 1往復 (eml が片側) | 4 以下 | 10.1 (echo-py) | echo-py-reuse の main の計 |
| many-self の 2000 本 ÷ 250 本の1接続あたり CPU | 1.5 以下 | 2.96 | many-self-* の単位あたり CPU |

- compio の上の1回の poll は、待ちの長さが 0 でも5回の syscall になる (`timerfd_settime`、`epoll_ctl(MOD timerfd)`、`epoll_pwait`、`read(eventfd) = EAGAIN`、`epoll_ctl(MOD eventfd)`)。
- 待つ `read_chunk` は1回 8 回である (Readable の `epoll_ctl(ADD, ONESHOT)`、poll の5回、`epoll_ctl(DEL)`、`recvfrom`)。両端が同じ run にあると、読み1回あたりの poll が2回になる。1回目は ADD の直後で空振りし、2回目は相手が書いた後に当たる。
- 読みにデータがもう来ている場合も、`Readable` を出して poll を通してから `Read` を出すので、数は変わらない (bulk-recv-py は `recvfrom` 1回あたり 9 回)。
- accept は `accept4` を先に試す。順に1本ずつの conn-self-1 では毎回 EAGAIN を受けて2回目で受け、32 本同時の conn-self-32 と many-self では1回で受ける。
- 監視のスレッド (`eml-monitor`) は `poll` と自己パイプの `read` を 10 ms に1回ずつ出す。

### 7.3 `RunStats` (`--stats`、床を引いて単位あたり)

床: parks 1、completions 1、tasks 1、allocations 34、string_bytes_copied 12、rc_increments 18、rc_decrements 51、preemptions 0。

| 件 | parks | completions | tasks | allocations | string_bytes_copied | rc_increments | rc_decrements | preemptions |
|---|---|---|---|---|---|---|---|---|
| echo-self-fresh | 4.00 | 6.00 | 0 | 76.06 | 16.02 | 44.03 | 120.08 | 0 |
| echo-self-reuse | 4.00 | 6.00 | 0 | 74.06 | 0.03 | 46.03 | 120.09 | 0 |
| echo-py-fresh | 2.00 | 3.00 | 0 | 42.02 | 16.04 | 22.00 | 64.02 | 0 |
| echo-py-reuse | 2.00 | 3.00 | 0 | 40.02 | 0.04 | 23.00 | 63.02 | 0 |
| bulk-self (MiB) | 32.04 | 48.04 | 0.01 | 609.34 | 1 048 577 | 352.51 | 961.85 | 0 |
| bulk-shared | 32.04 | 48.04 | 0.01 | 609.35 | 656 | 368.51 | 977.86 | 0 |
| bulk-reuse | 32.04 | 48.04 | 0.01 | 577.34 | 1 048 577 | 352.51 | 929.85 | 0 |
| bulk-1k | 2048.25 | 3072.25 | 0.06 | 38 920 | 1 048 581 | 22 531 | 61 452 | 0 |
| bulk-send-py | 16.00 | 16.00 | 0 | 304.60 | 1 048 577 | 176.00 | 480.60 | 0 |
| bulk-recv-py | 16.06 | 32.12 | 0 | 321.79 | 1.21 | 176.66 | 498.45 | 0 |
| bulk-accum | 32.25 | 48.25 | 0.06 | 600.38 | 1 048 581 | 355.19 | 955.56 | 2.31 |
| many-self-250 / 1000 / 2000 | 7.00 | 10.00 | 2.00 | 165 | 16 | 99 | 264 | 0 |
| many-py-1000 | 3.00 | 4.00 | 1.00 | 72.02 | 16.02 | 40.01 | 112.03 | 0 |
| conn-self-1 | 3.00 | 4.00 | 0 | 58.05 | 0.02 | 33.02 | 91.07 | 0 |
| conn-self-32 | 3.00 | 4.00 | 0.03 | 56.57 | 0.02 | 33.25 | 89.82 | 0 |
| conn-py | 2.00 | 3.00 | 0 | 40.01 | 0.04 | 21.99 | 62.00 | 0 |

- 待ち1回で `parks` が 1、書きの `completions` が 1、読みが 2 である。今は待ちのたびに park があり、`parks` は待ちの数と同じになる。
- bulk-recv-py の MiB あたりの `parks` と `completions` は、読み1回の量が run ごとに変わるので揺れる (1 MiB あたり 17.9 回から 19.8 回)。読み1回あたりの `completions` は 2 で変わらない。
- 共有の `Bytes` を書くと、基点の `submit_write` は2回写す (`to_vec` と `alloc_bytes`)。100 MiB の bulk-shared では、この写しが命令の 47.9% を占めた。この写しは `string_bytes_copied` に数えない。
- many-self の RSS は 250 本 18.8 MiB、1000 本 53 MiB、2000 本 222 MiB である。2000 本の massif では、ヒープの山 4.59 億バイトの 57% が `Registry::complete` からの読みの buffer の確保だった。本数で RSS が段になる理由は推論のままである (1回の `complete` で同時に読めるようになる読みが本数とともに増え、そのそれぞれに 64 KiB 以上を確保する)。
- many-self の callgrind では、1000 本で命令の 78.8%、2000 本で 90.7% が compio の上の `RealDriver::reap` の中にあった。`reap` からの `HashMap::insert` は 1000 本で 208 万回、2000 本で 863 万回で、命令の数は 3.6 倍と本数の2乗に近い。

## 8. ファイルの全文

### 8.1 `bench/net/cases.tsv` (T-2)

区切りはタブである。行の順は流す順になる。`compute-fib` の行は T-2 で足す行で、`busy-self` の行は T-9 が末尾の `py:` の行の前に足す (8.4 節)。

```
# 物差しの件。1行1件で、タブで区切る。足すのも外すのも行の足し引きで済む。
# 名前	プログラム	時間の引数	小さい引数	相手 (時間)	相手 (小さい)	時間の単位の数	小さい単位の数	単位
# 相手が - なら同じ run の中で両端を動かす。`py:` で始まる名前は eml でなく Python 同士の床で、時間だけを測る
base	base.em	-	-	-	-	1	1	run
compute-fib	../fib.em	-	-	-	-	1	1	run
echo-self-fresh	echo.em	20000 16 fresh	2000 16 fresh	-	-	20000	2000	往復
echo-self-reuse	echo.em	20000 16 reuse	2000 16 reuse	-	-	20000	2000	往復
echo-py-fresh	echo.em	20000 16 fresh {port}	2000 16 fresh {port}	echo-server	echo-server	20000	2000	往復
echo-py-reuse	echo.em	20000 16 reuse {port}	2000 16 reuse {port}	echo-server	echo-server	20000	2000	往復
bulk-self	bulk.em	1024 64 fresh fresh	100 64 fresh fresh	-	-	1024	100	MiB
bulk-shared	bulk.em	1024 64 shared fresh	100 64 shared fresh	-	-	1024	100	MiB
bulk-reuse	bulk.em	1024 64 fresh reuse	100 64 fresh reuse	-	-	1024	100	MiB
bulk-1k	bulk.em	64 1 fresh fresh	16 1 fresh fresh	-	-	64	16	MiB
bulk-send-py	bulk.em	1024 64 fresh fresh {port}	100 64 fresh fresh {port}	sink-server	sink-server	1024	100	MiB
bulk-recv-py	bulk.em	1024 64 fresh fresh -{port}	100 64 fresh fresh -{port}	source-server 1024 64	source-server 100 64	1024	100	MiB
bulk-accum	bulk.em	16 64 fresh accum	16 64 fresh accum	-	-	16	16	MiB
many-self-250	many.em	250	250	-	-	250	250	接続
many-self-1000	many.em	1000	1000	-	-	1000	1000	接続
many-self-2000	many.em	2000	2000	-	-	2000	2000	接続
many-py-1000	many.em	1000 {port}	1000 {port}	many-server 1000	many-server 1000	1000	1000	接続
conn-self-1	conn.em	10000 1	1000 1	-	-	10000	1000	接続
conn-self-32	conn.em	10240 32	1024 32	-	-	10240	1024	接続
conn-py	conn.em	10000 1 {port}	1000 1 {port}	conn-server 10000	conn-server 1000	10000	1000	接続
py:echo	-	echo-client 20000 16 {port}	-	echo-server	-	20000	0	往復
py:bulk	-	bulk-send 1024 64 {port}	-	sink-server	-	1024	0	MiB
py:conn	-	conn-client 10000 {port}	-	conn-server 10000	-	10000	0	接続
```

### 8.2 プログラムと script (T-2)

#### `bench/net/base.em`

床の run。

```haskell
-- 物差しの床。Net と Async を読み込んで、何もしない run を1回する (コンパイルと起動の分)
import Async (Async)
import Async
import Bytes
import Net
import Env
import String

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let l = Net.listen "127.0.0.1" 0
  Net.close_listener l
  println "base done \{show (int_arg 0 0)}")

-- 引数の読み取り。各プログラムが1ファイルで動くように、同じ定義を写して持つ
int_arg : Int -> Int -> <IO, Panic> Int
int_arg i default =
  match nth i (Env.args ()) with
    | Some s ->
      match String.to_int s with
        | Some n -> n
        | None -> default
    | None -> default

str_arg : Int -> String -> <IO, Panic> String
str_arg i default =
  match nth i (Env.args ()) with
    | Some s -> s
    | None -> default

nth : Int -> List String -> <Panic> Option String
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else nth (i - 1) rest
```

#### `bench/net/echo.em`

1本の接続の往復。`write_bytes` の型は T-2 の時点の形 (`TcpStream` を返す) で書いてあり、T-8 が 8.3 節の差分を当てる。

```haskell
-- (a) ループバックの echo。1 本の接続で `size` バイトの往復を `n` 回する。
-- 引数: n size mode [port]
--   mode: fresh なら書くたびに Bytes.make で作り、読むたびに Bytes.empty () を渡す。reuse なら1つの buffer を
--         Bytes.truncate 0 で空にして読みに渡し直し、書きには同じ buffer を渡す (共有なので submit_write が写す)
--   port: 0 なら同じ run の中のタスクが相手になる。正なら 127.0.0.1:port の echo サーバーへ接続する
import Async (Async)
import Async
import Bytes
import Env
import Net
import String

serve_fresh : Net.TcpStream -> <IO, Panic, Async> Unit
serve_fresh s =
  let (buf, s) = Net.read_chunk (Bytes.empty ()) s
  if Bytes.length buf == 0 then Net.close s
  else serve_fresh (Net.write_bytes buf s)

serve_reuse : Bytes -> Net.TcpStream -> <IO, Panic, Async> Unit
serve_reuse b s =
  let (b, s) = Net.read_chunk (Bytes.truncate 0 b) s
  if Bytes.length b == 0 then Net.close s
  else serve_reuse b (Net.write_bytes b s)

-- `want` バイト集まるまで読む
read_exact : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> (Bytes, Net.TcpStream)
read_exact want b s =
  if Bytes.length b >= want then (b, s)
  else
    let (b, s) = Net.read_chunk b s
    read_exact want b s

rounds_fresh : Int -> Int -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
rounds_fresh n size s =
  if n == 0 then s
  else
    let s = Net.write_bytes (Bytes.make size 'x'u8) s
    let (_, s) = read_exact size (Bytes.empty ()) s
    rounds_fresh (n - 1) size s

rounds_reuse : Int -> Int -> Bytes -> Bytes -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
rounds_reuse n size msg b s =
  if n == 0 then s
  else
    let s = Net.write_bytes msg s
    let (b, s) = read_exact size (Bytes.truncate 0 b) s
    rounds_reuse (n - 1) size msg b s

client : Int -> Int -> String -> Int -> <IO, Panic, Async> Unit
client n size mode port =
  let c = Net.set_nodelay True (Net.connect "127.0.0.1" port)
  if mode == "reuse" then Net.close (rounds_reuse n size (Bytes.make size 'x'u8) (Bytes.empty ()) c)
  else Net.close (rounds_fresh n size c)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let n = int_arg 0 10000
  let size = int_arg 1 16
  let mode = str_arg 2 "fresh"
  let port = int_arg 3 0
  if port == 0 then
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let t = Async.start (fn () ->
      let (s, l) = Net.accept l
      Net.close_listener l
      let s = Net.set_nodelay True s
      if mode == "reuse" then serve_reuse (Bytes.empty ()) s else serve_fresh s)
    client n size mode port
    Async.await t
    println "echo done \{show n}"
  else
    client n size mode port
    println "echo done \{show n}")

-- 引数の読み取り。各プログラムが1ファイルで動くように、同じ定義を写して持つ
int_arg : Int -> Int -> <IO, Panic> Int
int_arg i default =
  match nth i (Env.args ()) with
    | Some s ->
      match String.to_int s with
        | Some n -> n
        | None -> default
    | None -> default

str_arg : Int -> String -> <IO, Panic> String
str_arg i default =
  match nth i (Env.args ()) with
    | Some s -> s
    | None -> default

nth : Int -> List String -> <Panic> Option String
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else nth (i - 1) rest
```

#### `bench/net/bulk.em`

大きい転送。

```haskell
-- (b) 大きい転送。1 本の接続で `mib` MiB を `chunk` KiB ずつ write_bytes で送り、相手は read_chunk で EOF まで読む。
-- 引数: mib chunk_kib wmode rmode [port]
--   wmode: fresh なら書くたびに Bytes.make で新しい buffer を作る、shared なら同じ buffer を何度も渡す (共有なので submit_write が写す)
--   rmode: fresh なら読むたびに Bytes.empty () を渡す、reuse なら返った buffer を truncate 0 で空にして渡し直す、
--          accum なら1つの buffer に足し続ける
--   port: 0 なら同じ run の中で送り手と読み手を動かす。正なら送り手だけになり 127.0.0.1:port へ送る。負なら読み手だけになり
--         127.0.0.1:(-port) へ接続して EOF まで読む
import Async (Async)
import Async
import Bytes
import Net
import Env
import String

drain_fresh : Int -> Net.TcpStream -> <IO, Panic, Async> Int
drain_fresh total s =
  let (b, s) = Net.read_chunk (Bytes.empty ()) s
  let n = Bytes.length b
  if n == 0 then
    Net.close s
    total
  else drain_fresh (total + n) s

drain_reuse : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> Int
drain_reuse total b s =
  let (b, s) = Net.read_chunk (Bytes.truncate 0 b) s
  let n = Bytes.length b
  if n == 0 then
    Net.close s
    total
  else drain_reuse (total + n) b s

drain_accum : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> Int
drain_accum before b s =
  let (b, s) = Net.read_chunk b s
  let n = Bytes.length b
  if n == before then
    Net.close s
    n
  else drain_accum n b s

drain : String -> Net.TcpStream -> <IO, Panic, Async> Int
drain rmode s =
  if rmode == "reuse" then drain_reuse 0 (Bytes.empty ()) s
  else if rmode == "accum" then drain_accum 0 (Bytes.empty ()) s
  else drain_fresh 0 s

send_fresh : Int -> Int -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
send_fresh n size s =
  if n == 0 then s else send_fresh (n - 1) size (Net.write_bytes (Bytes.make size 'x'u8) s)

send_shared : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
send_shared n b s =
  if n == 0 then s else send_shared (n - 1) b (Net.write_bytes b s)

send : String -> Int -> Int -> Net.TcpStream -> <IO, Panic, Async> Unit
send wmode n size s =
  if wmode == "shared" then Net.close (send_shared n (Bytes.make size 'x'u8) s)
  else Net.close (send_fresh n size s)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let mib = int_arg 0 100
  let size = int_arg 1 64 * 1024
  let wmode = str_arg 2 "fresh"
  let rmode = str_arg 3 "fresh"
  let port = int_arg 4 0
  let n = mib * 1024 * 1024 / size
  if port == 0 then
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let t = Async.start (fn () ->
      let (s, l) = Net.accept l
      Net.close_listener l
      drain rmode s)
    send wmode n size (Net.connect "127.0.0.1" port)
    let got = Async.await t
    println "bulk done \{show got}"
  else if port > 0 then
    send wmode n size (Net.connect "127.0.0.1" port)
    println "bulk sent \{show (n * size)}"
  else
    let got = drain rmode (Net.connect "127.0.0.1" (0 - port))
    println "bulk done \{show got}")

-- 引数の読み取り。各プログラムが1ファイルで動くように、同じ定義を写して持つ
int_arg : Int -> Int -> <IO, Panic> Int
int_arg i default =
  match nth i (Env.args ()) with
    | Some s ->
      match String.to_int s with
        | Some n -> n
        | None -> default
    | None -> default

str_arg : Int -> String -> <IO, Panic> String
str_arg i default =
  match nth i (Env.args ()) with
    | Some s -> s
    | None -> default

nth : Int -> List String -> <Panic> Option String
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else nth (i - 1) rest
```

#### `bench/net/many.em`

多数の接続。

```haskell
-- (c) 多数の接続。クライアントは `c` 本の接続を1本ずつ順に張り (同時に backlog に入るのは1本まで)、張り終えてから
-- 接続ごとのタスクで 16 バイトの要求を書いて返事を読み、閉じる。サーバーは受けた接続ごとにすぐタスクを始め、要求を
-- 読んで返し、相手の EOF を読んで閉じる。張る間はサーバーの側に読みが張った本数だけ保留になり、張り終えた後は
-- `c` 本の読みがほぼ同時に読めるようになる。
-- 引数: c [port]。port が 0 なら同じ run の中でサーバーとクライアントを動かす。正ならクライアントだけで 127.0.0.1:port へ
import Async (Async)
import Async
import Bytes
import Env
import Net
import String

answer : Net.TcpStream -> <IO, Panic, Async> Unit
answer s =
  let (req, s) = Net.read_chunk (Bytes.empty ()) s
  let s = Net.write_bytes req s
  let (_, s) = Net.read_chunk (Bytes.empty ()) s
  Net.close s

accept_loop : Async.Scope -> Int -> Net.TcpListener -> <IO, Panic, Async> Unit
accept_loop sc n l =
  if n == 0 then Net.close_listener l
  else
    let (s, l) = Net.accept l
    Async.spawn_in sc (fn () -> answer s)
    accept_loop sc (n - 1) l

connect_all : Int -> Int -> List Net.TcpStream -> <IO, Panic, Async> List Net.TcpStream
connect_all n port acc =
  if n == 0 then acc else connect_all (n - 1) port (Net.connect "127.0.0.1" port :: acc)

ask : Net.TcpStream -> <IO, Panic, Async> Unit
ask c =
  let c = Net.write_bytes (Bytes.make 16 'q'u8) c
  let (_, c) = Net.read_chunk (Bytes.empty ()) c
  Net.close c

ask_all : Async.Scope -> List Net.TcpStream -> <IO, Panic, Async> Unit
ask_all sc xs =
  match xs with
    | Nil -> ()
    | c :: rest ->
      Async.spawn_in sc (fn () -> ask c)
      ask_all sc rest

clients : Int -> Int -> <IO, Panic, Async> Unit
clients c port =
  let xs = connect_all c port Nil
  Async.scope (fn sc -> ask_all sc xs)

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let c = int_arg 0 1000
  let port = int_arg 1 0
  if port == 0 then
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let server = Async.start (fn () -> Async.scope (fn sc -> accept_loop sc c l))
    clients c port
    Async.await server
    println "many done \{show c}"
  else
    clients c port
    println "many done \{show c}")

-- 引数の読み取り。各プログラムが1ファイルで動くように、同じ定義を写して持つ
int_arg : Int -> Int -> <IO, Panic> Int
int_arg i default =
  match nth i (Env.args ()) with
    | Some s ->
      match String.to_int s with
        | Some n -> n
        | None -> default
    | None -> default

str_arg : Int -> String -> <IO, Panic> String
str_arg i default =
  match nth i (Env.args ()) with
    | Some s -> s
    | None -> default

nth : Int -> List String -> <Panic> Option String
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else nth (i - 1) rest
```

#### `bench/net/conn.em`

accept と connect の速さ。

```haskell
-- (d) accept と connect の速さ。`par` 個のタスクが、それぞれ接続を張って閉じるのを順に `n / par` 回する。サーバーは
-- 受けたらすぐ閉じ、クライアントは EOF を読んでから閉じる。EOF を待つのは、クライアントが backlog を埋めて SYN の
-- 再送 (1 秒) を待つのを避けるためで、同時に張る接続は `par` 本までになる
-- 引数: n par [port]。port が 0 なら同じ run の中でサーバーを動かす。正ならクライアントだけで 127.0.0.1:port へ
import Async (Async)
import Async
import Bytes
import Env
import Net
import String

accept_loop : Int -> Net.TcpListener -> <IO, Panic, Async> Unit
accept_loop n l =
  if n == 0 then Net.close_listener l
  else
    let (s, l) = Net.accept l
    Net.close s
    accept_loop (n - 1) l

connect_loop : Int -> Int -> <IO, Panic, Async> Unit
connect_loop n port =
  if n == 0 then ()
  else
    let (_, c) = Net.read_chunk (Bytes.empty ()) (Net.connect "127.0.0.1" port)
    Net.close c
    connect_loop (n - 1) port

clients : Async.Scope -> Int -> Int -> Int -> <IO, Panic, Async> Unit
clients sc par each port =
  if par == 0 then ()
  else
    Async.spawn_in sc (fn () -> connect_loop each port)
    clients sc (par - 1) each port

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let n = int_arg 0 10000
  let par = int_arg 1 1
  let port = int_arg 2 0
  let each = n / par
  if port == 0 then
    let l = Net.listen "127.0.0.1" 0
    let (port, l) = Net.local_port l
    let t = Async.start (fn () -> accept_loop (each * par) l)
    if par == 1 then connect_loop each port else Async.scope (fn sc -> clients sc par each port)
    Async.await t
    println "conn done \{show (each * par)}"
  else
    if par == 1 then connect_loop each port else Async.scope (fn sc -> clients sc par each port)
    println "conn done \{show (each * par)}")

-- 引数の読み取り。各プログラムが1ファイルで動くように、同じ定義を写して持つ
int_arg : Int -> Int -> <IO, Panic> Int
int_arg i default =
  match nth i (Env.args ()) with
    | Some s ->
      match String.to_int s with
        | Some n -> n
        | None -> default
    | None -> default

str_arg : Int -> String -> <IO, Panic> String
str_arg i default =
  match nth i (Env.args ()) with
    | Some s -> s
    | None -> default

nth : Int -> List String -> <Panic> Option String
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else nth (i - 1) rest
```

#### `bench/net/peer.py`

Python の相手。

```python
"""計測の相手 (ブロックする socket の Python)。サーバーは待ち受けたポートを標準出力の1行目に出す。

python3 -I peer.py echo-server
python3 -I peer.py sink-server
python3 -I peer.py source-server MIB CHUNK_KIB
python3 -I peer.py many-server C
python3 -I peer.py conn-server N
python3 -I peer.py echo-client N SIZE PORT     (Python 同士の床を測るためのクライアント)
python3 -I peer.py bulk-send MIB CHUNK_KIB PORT
python3 -I peer.py conn-client N PORT
"""
import socket
import sys


def listener():
    l = socket.socket()
    l.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    l.bind(("127.0.0.1", 0))
    l.listen(4096)
    print(l.getsockname()[1], flush=True)
    return l


def nodelay(s):
    s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    return s


def main():
    mode, args = sys.argv[1], [int(a) for a in sys.argv[2:]]
    if mode == "echo-server":
        s, _ = listener().accept()
        nodelay(s)
        while True:
            b = s.recv(65536)
            if not b:
                break
            s.sendall(b)
        s.close()
    elif mode == "sink-server":
        s, _ = listener().accept()
        while s.recv(1 << 20):
            pass
        s.close()
    elif mode == "source-server":
        mib, kib = args
        s, _ = listener().accept()
        buf = b"x" * (kib * 1024)
        for _ in range(mib * 1024 // kib):
            s.sendall(buf)
        s.close()
    elif mode == "many-server":
        (c,) = args
        l = listener()
        xs = [l.accept()[0] for _ in range(c)]
        for s in xs:
            s.sendall(s.recv(65536))
        for s in xs:
            s.recv(65536)
            s.close()
    elif mode == "conn-server":
        (n,) = args
        l = listener()
        for _ in range(n):
            l.accept()[0].close()
    elif mode == "echo-client":
        n, size, port = args
        s = nodelay(socket.create_connection(("127.0.0.1", port)))
        msg = b"x" * size
        for _ in range(n):
            s.sendall(msg)
            got = 0
            while got < size:
                got += len(s.recv(65536))
        s.close()
    elif mode == "bulk-send":
        mib, kib, port = args
        s = socket.create_connection(("127.0.0.1", port))
        buf = b"x" * (kib * 1024)
        for _ in range(mib * 1024 // kib):
            s.sendall(buf)
        s.close()
    elif mode == "conn-client":
        n, port = args
        for _ in range(n):
            s = socket.create_connection(("127.0.0.1", port))
            s.recv(1)
            s.close()
    else:
        sys.exit(f"unknown mode {mode}")


main()
```

#### `bench/net/run.py`

1件を流して JSON を出す。

```python
"""1 つの物差しを流し、測った値を JSON の1行 (run ごとの辞書のリスト) で出す。

python3 -I run.py [--peer 'MODE ARGS'] [--strace] [--runs R] [--pin CPUS] [--peer-pin CPUS] -- CMD ARGS...

CMD の引数の `{port}` は、相手 (peer.py) が出したポートに置き換える。
--strace がなければ、実時間、user と sys の CPU、最大の常駐メモリ (KiB)、自発と非自発の切り替えを測る。
--strace なら `strace -f` の下で流し、スレッドごと (main、eml-monitor など PR_SET_NAME の名前、ほかは thread) に
syscall の名前ごとの数と、失敗の数 (`名前!ERRNO`) を出す。eventfd と timerfd の `read` は `read(eventfd)` と
`read(timerfd)` に分ける。時間は歪むので記録しない。Linux でだけ使える。
CMD の標準エラーに `stats: name=value ...` の行 (`eml run --stats`) があれば、その欄をすべて出す。
--pin は CMD を、--peer-pin は相手を `taskset -c` で CPU に固定する (Linux)。
"""
import json
import os
import re
import subprocess
import sys
import tempfile
import time

here = os.path.dirname(os.path.abspath(__file__))
darwin = sys.platform == "darwin"


def parse():
    opts = {"peer": None, "strace": False, "runs": 1, "pin": None, "peer_pin": None}
    argv = sys.argv[1:]
    while argv and argv[0] != "--":
        a = argv.pop(0)
        if a == "--peer":
            opts["peer"] = argv.pop(0)
        elif a == "--strace":
            opts["strace"] = True
        elif a == "--runs":
            opts["runs"] = int(argv.pop(0))
        elif a == "--pin":
            opts["pin"] = argv.pop(0)
        elif a == "--peer-pin":
            opts["peer_pin"] = argv.pop(0)
        else:
            sys.exit(f"unknown option {a}")
    if opts["strace"] and darwin:
        sys.exit("--strace は Linux でだけ使える")
    if (opts["pin"] or opts["peer_pin"]) and darwin:
        sys.exit("--pin は Linux でだけ使える")
    return opts, argv[1:]


def pinned(cpus, cmd):
    return ["taskset", "-c", cpus] + cmd if cpus else cmd


def start_peer(spec, cpus):
    cmd = pinned(cpus, [sys.executable, "-I", os.path.join(here, "peer.py")] + spec.split())
    p = subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True)
    port = p.stdout.readline().strip()
    if not port:
        sys.exit(f"peer did not start: {spec}")
    return p, port


START = re.compile(r"^(\d+)\s+(\w+)\((.*)$")
RESUMED = re.compile(r"^(\d+)\s+<\.\.\. (\w+) resumed>(.*)$")
RESULT = re.compile(r"=\s+(-?\d+|\?)(?:\s+(E[A-Z0-9]+))?")


def strace_counts(path):
    main = None
    names = {}
    eventfds, timerfds = set(), set()
    counts = {}
    pending = {}

    def bump(tid, key):
        d = counts.setdefault(tid, {})
        d[key] = d.get(key, 0) + 1

    def result(tid, name, rest, args):
        m = RESULT.search(rest)
        if not m:
            return
        ret, err = m.group(1), m.group(2)
        if err:
            bump(tid, f"{name}!{err}")
        elif name == "eventfd2":
            eventfds.add(ret)
        elif name == "timerfd_create":
            timerfds.add(ret)

    for line in open(path):
        m = RESUMED.match(line)
        if m:
            tid, name, rest = m.groups()
            result(tid, pending.pop(tid, name), rest, "")
            continue
        m = START.match(line)
        if not m:
            continue
        tid, name, rest = m.groups()
        if main is None:
            main = tid
        if name == "prctl":
            n = re.search(r'PR_SET_NAME, "([^"]*)"', rest)
            if n:
                names[tid] = n.group(1)
        key = name
        if name == "read":
            fd = rest.split(",", 1)[0].strip()
            if fd in eventfds:
                key = "read(eventfd)"
            elif fd in timerfds:
                key = "read(timerfd)"
        bump(tid, key)
        if "<unfinished ...>" in rest:
            pending[tid] = key
        else:
            result(tid, key, rest, rest)
    # 名前は PR_SET_NAME の後で分かるので、スレッドの番号で数えてから名前にまとめる
    out = {}
    for tid, d in counts.items():
        who = "main" if tid == main else names.get(tid, "thread")
        o = out.setdefault(who, {})
        for k, v in d.items():
            o[k] = o.get(k, 0) + v
    return out


def once(opts, cmd):
    peer, port = (None, None)
    if opts["peer"]:
        peer, port = start_peer(opts["peer"], opts["peer_pin"])
    cmd = [c.replace("{port}", port or "") for c in cmd]
    trace = None
    if opts["strace"]:
        fd, trace = tempfile.mkstemp(suffix=".strace")
        os.close(fd)
        cmd = ["strace", "-f", "-qq", "-o", trace] + cmd
    cmd = pinned(opts["pin"], cmd)
    errf = tempfile.TemporaryFile(mode="w+")
    t0 = time.perf_counter()
    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=errf)
    _, status, ru = os.wait4(p.pid, 0)
    wall = time.perf_counter() - t0
    if peer:
        peer.wait(timeout=60)
    errf.seek(0)
    err = errf.read()
    code = os.waitstatus_to_exitcode(status)
    if code != 0:
        sys.exit(f"failed ({code}): {' '.join(cmd)}\n{err}")
    # macOS の ru_maxrss はバイト、Linux は KiB
    rss = ru.ru_maxrss // 1024 if darwin else ru.ru_maxrss
    out = {"wall": wall, "user": ru.ru_utime, "sys": ru.ru_stime, "maxrss_kib": rss,
           "nvcsw": ru.ru_nvcsw, "nivcsw": ru.ru_nivcsw, "minflt": ru.ru_minflt}
    m = re.search(r"^stats: (.*)$", err, re.M)
    if m:
        out["stats"] = {k: int(v) for k, v in re.findall(r"(\w+)=(\d+)", m.group(1))}
    if trace:
        out = {"syscalls": strace_counts(trace)}
        os.unlink(trace)
    return out


def main():
    opts, cmd = parse()
    print(json.dumps([once(opts, cmd) for _ in range(opts["runs"])]))


main()
```

#### `bench/net/bench.py`

`cases.tsv` を流して JSONL を出す。

```python
"""本物の I/O の物差しを流し、1行1件の JSON (JSONL) を標準出力に出す。

python3 -I bench.py [--runs 11] [--strace-runs 3] [--stats] [--only REGEX] [--no-time] [--no-strace] LABEL=EML ...

LABEL=EML を2つ以上渡すと、時間の run は件ごとに交互に (1回目は A B、2回目は B A、…) 流す。機械の揺れを片方に
寄せないためである。--stats は `EML run --stats` (検査モード) を1回流して RunStats を取る。件は cases.tsv の行である。
"""
import json
import os
import re
import subprocess
import sys

here = os.path.dirname(os.path.abspath(__file__))


def cases():
    for line in open(os.path.join(here, "cases.tsv")):
        if line.startswith("#") or not line.strip():
            continue
        f = line.rstrip("\n").split("\t")
        yield dict(name=f[0], prog=f[1], big=f[2], small=f[3], peer_big=f[4], peer_small=f[5],
                   n_big=int(f[6]), n_small=int(f[7]), unit=f[8])


def run(extra, cmd):
    r = subprocess.run([sys.executable, "-I", os.path.join(here, "run.py")] + extra + ["--"] + cmd,
                       capture_output=True, text=True, timeout=600)
    if r.returncode != 0:
        sys.exit(r.stderr or r.stdout)
    return json.loads(r.stdout)


def command(case, eml, size, stats=False):
    args = [] if case[size] == "-" else case[size].split()
    if case["name"].startswith("py:"):
        return [sys.executable, "-I", os.path.join(here, "peer.py")] + args
    return [eml, "run"] + (["--stats"] if stats else []) + [os.path.join(here, case["prog"])] + args


def peer(case, size):
    p = case["peer_" + size]
    return [] if p == "-" else ["--peer", p]


def main():
    argv = sys.argv[1:]
    runs, strace_runs, stats, only, do_time, do_strace = 11, 3, False, None, True, True
    emls = []
    while argv:
        a = argv.pop(0)
        if a == "--runs":
            runs = int(argv.pop(0))
        elif a == "--strace-runs":
            strace_runs = int(argv.pop(0))
        elif a == "--stats":
            stats = True
        elif a == "--only":
            only = re.compile(argv.pop(0))
        elif a == "--no-time":
            do_time = False
        elif a == "--no-strace":
            do_strace = False
        else:
            label, eml = a.split("=", 1)
            emls.append((label, os.path.abspath(eml)))
    if not emls:
        sys.exit(__doc__)
    # `--stats` を持たない古い eml では RunStats を取らずに続ける
    has_stats = {label: "--stats" in subprocess.run([eml, "run", "--help"], capture_output=True, text=True).stdout
                 for label, eml in emls}
    for label, ok in has_stats.items():
        if stats and not ok:
            print(f"bench.py: {label} の eml に --stats がないので、RunStats を取らない", file=sys.stderr)

    def emit(case, label, kind, data):
        print(json.dumps({"case": case["name"], "label": label, "kind": kind, "unit": case["unit"],
                          "n_big": case["n_big"], "n_small": case["n_small"], "data": data}), flush=True)

    for case in cases():
        if only and not only.search(case["name"]):
            continue
        py = case["name"].startswith("py:")
        labels = emls[:1] if py else emls
        if do_time:
            got = {label: [] for label, _ in labels}
            for i in range(runs):
                order = labels if i % 2 == 0 else labels[::-1]
                for label, eml in order:
                    got[label] += run(peer(case, "big"), command(case, eml, "big"))
            for label, _ in labels:
                emit(case, "python" if py else label, "time", got[label])
        if py:
            continue
        for label, eml in labels:
            if do_strace and sys.platform.startswith("linux"):
                emit(case, label, "strace", run(peer(case, "small") + ["--strace", "--runs", str(strace_runs)],
                                                command(case, eml, "small")))
            if stats and has_stats[label]:
                emit(case, label, "stats", run(peer(case, "small"), command(case, eml, "small", stats=True)))


main()
```

#### `bench/net/report.py`

JSONL を Markdown の表にする。

```python
"""bench.py の JSONL を Markdown の表にする。ラベルが2つあれば、1つ目を前、2つ目を後として比べる表も出す。

python3 -I report.py results.jsonl

比べる規則 (docs/implementation/benchmarks.md の「ネットワークの計測」): 主指標は eml のプロセスの CPU (user+sys) を床を引いて単位で割った値の中央値。後÷前の比が
0.90 以下か 1.10 以上で、かつ生の CPU の四分位の範囲 (25%-75%) が重ならないときだけ「差」とする。
"""
import json
import statistics as st
import sys

rows = [json.loads(l) for l in open(sys.argv[1])]
labels = []
for r in rows:
    if r["label"] not in labels and r["label"] != "python":
        labels.append(r["label"])
get = {(r["case"], r["label"], r["kind"]): r for r in rows}
cases = []
for r in rows:
    if r["case"] not in cases:
        cases.append(r["case"])

SYS = ["epoll_ctl", "epoll_pwait", "epoll_wait", "timerfd_settime", "read(eventfd)", "recvfrom", "recvfrom!EAGAIN",
       "sendto", "accept4", "accept4!EAGAIN", "connect", "brk", "mmap", "munmap"]
STATS = ["parks", "completions", "tasks", "allocations", "string_bytes_copied", "rc_increments",
         "rc_decrements", "preemptions"]


def cpu(r):
    return r["user"] + r["sys"]


def q(xs):
    xs = sorted(xs)
    if len(xs) < 4:
        return xs[0], xs[-1]
    a = st.quantiles(xs, n=4)
    return a[0], a[2]


def base_med(label, f):
    b = get.get(("base", label, "time"))
    return st.median(f(x) for x in b["data"]) if b else 0


def fmt(x):
    return f"{x:.2f}" if abs(x) >= 0.005 else "0"


for label in labels + ["python"]:
    print(f"\n## 時間: {label} (中央値。括弧は最小-最大。単位あたりは床を引く。µs)\n")
    print("| 件 | 単位 | 実時間 s | CPU s (四分位) | 単位あたり 実時間 | 単位あたり CPU | 単位あたり sys | 最大 RSS MiB | minflt/単位 | 自発/非自発 |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for c in cases:
        r = get.get((c, label, "time"))
        if not r:
            continue
        d, n = r["data"], r["n_big"]
        floor = c != "base" and label != "python"
        bw = base_med(label, lambda x: x["wall"]) if floor else 0
        bc = base_med(label, cpu) if floor else 0
        bs = base_med(label, lambda x: x["sys"]) if floor else 0
        bf = base_med(label, lambda x: x["minflt"]) if floor else 0
        w = [x["wall"] for x in d]
        cs = [cpu(x) for x in d]
        lo, hi = q(cs)
        print(f"| {c} | {n} {r['unit']} | {st.median(w):.3f} ({min(w):.3f}-{max(w):.3f}) | {st.median(cs):.3f} ({lo:.3f}-{hi:.3f}) | "
              f"{(st.median(w) - bw) / n * 1e6:.2f} | {(st.median(cs) - bc) / n * 1e6:.2f} | "
              f"{(st.median(x['sys'] for x in d) - bs) / n * 1e6:.2f} | {st.median(x['maxrss_kib'] for x in d) / 1024:.1f} | "
              f"{(st.median(x['minflt'] for x in d) - bf) / n:.2f} | "
              f"{int(st.median(x['nvcsw'] for x in d))}/{int(st.median(x['nivcsw'] for x in d))} |")

for label in labels:
    b = get.get(("base", label, "strace"))
    if not b:
        continue
    print(f"\n## syscall: {label} (strace -f、main のスレッド、床を引いて単位あたり。複数回なら最小-最大)\n")
    print("| 件 | 単位 | " + " | ".join(SYS) + " | main の計 | ほかのスレッドの計 |")
    print("|---|---|" + "---|" * (len(SYS) + 2))
    bm = b["data"][0]["syscalls"].get("main", {})
    bo = sum(sum(v.values()) for k, v in b["data"][0]["syscalls"].items() if k != "main")
    for c in cases:
        r = get.get((c, label, "strace"))
        if not r or c == "base":
            continue
        n = r["n_small"]
        cells = []
        for k in SYS + ["#main", "#other"]:
            vals = []
            for x in r["data"]:
                m = x["syscalls"].get("main", {})
                if k == "#main":
                    v = sum(m.values()) - sum(bm.values())
                elif k == "#other":
                    v = sum(sum(t.values()) for name, t in x["syscalls"].items() if name != "main") - bo
                else:
                    v = m.get(k, 0) - bm.get(k, 0)
                vals.append(v / n)
            lo, hi = min(vals), max(vals)
            cells.append(fmt(lo) if abs(hi - lo) < 0.005 else f"{fmt(lo)}-{fmt(hi)}")
        print(f"| {c} | {n} {r['unit']} | " + " | ".join(cells) + " |")

for label in labels:
    b = get.get(("base", label, "stats"))
    if not b:
        continue
    bs = b["data"][0]["stats"]
    present = [k for k in STATS if k in bs]
    print(f"\n## RunStats: {label} (--stats、床を引いて単位あたり。床: " + ", ".join(f"{k}={bs[k]}" for k in present) + ")\n")
    print("| 件 | 単位 | " + " | ".join(present) + " |")
    print("|---|---|" + "---|" * len(present))
    for c in cases:
        r = get.get((c, label, "stats"))
        if not r or c == "base":
            continue
        s = r["data"][0]["stats"]
        print(f"| {c} | {r['n_small']} {r['unit']} | " + " | ".join(fmt((s[k] - bs[k]) / r["n_small"]) for k in present) + " |")

if len(labels) >= 2:
    a, b = labels[0], labels[1]
    print(f"\n## 比べ: {a} → {b} (CPU の中央値、床を引いて単位あたり µs)\n")
    print(f"| 件 | {a} | {b} | 比 | 四分位の重なり | 判定 | 実時間の比 |")
    print("|---|---|---|---|---|---|---|")
    for c in cases:
        ra, rb = get.get((c, a, "time")), get.get((c, b, "time"))
        if not ra or not rb or c == "base":
            continue
        n = ra["n_big"]
        pa = (st.median(cpu(x) for x in ra["data"]) - base_med(a, cpu)) / n * 1e6
        pb = (st.median(cpu(x) for x in rb["data"]) - base_med(b, cpu)) / n * 1e6
        qa, qb = q([cpu(x) for x in ra["data"]]), q([cpu(x) for x in rb["data"]])
        overlap = not (qa[1] < qb[0] or qb[1] < qa[0])
        ratio = pb / pa if pa else float("nan")
        verdict = "差" if (ratio <= 0.90 or ratio >= 1.10) and not overlap else "差なし"
        wa = st.median(x["wall"] for x in ra["data"]) - base_med(a, lambda x: x["wall"])
        wb = st.median(x["wall"] for x in rb["data"]) - base_med(b, lambda x: x["wall"])
        print(f"| {c} | {pa:.2f} | {pb:.2f} | {ratio:.2f} | {'重なる' if overlap else '重ならない'} | {verdict} | {wb / wa:.2f} |")
```

#### `bench/net.sh`

実行できる印 (`chmod +x`) を付けて入れる。

```bash
#!/usr/bin/env bash
# Std.Net の物差し (bench/net/cases.tsv の件) を release ビルドの eml で流し、bench/net/bench.py の JSONL を標準出力に出す
# (docs/implementation/benchmarks.md の「ネットワークの計測」)。引数は bench.py にそのまま渡し、比べる eml は LABEL=EML で渡す
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo build --release -p eml_cli --manifest-path "$root/Cargo.toml" >&2
exec python3 -I "$root/bench/net/bench.py" "$@"
```

### 8.3 T-8 が当てる差分 (`write_bytes` が `(Bytes, TcpStream)` を返す形)

件の意味は変えない。`shared` の件は同じ `Bytes` を2つの所で使い続け、共有のまま書く。`reuse` の件は `write_bytes` が返した buffer を次の往復でも使う。

`bench/net/echo.em`:

```haskell
--   mode: fresh なら書くたびに Bytes.make で作り、読むたびに Bytes.empty () を渡す。reuse なら1つの buffer を
--         Bytes.truncate 0 で空にして読みに渡し直し、書きには write_bytes が返した buffer を次の往復でも渡す

serve_fresh : Net.TcpStream -> <IO, Panic, Async> Unit
serve_fresh s =
  let (buf, s) = Net.read_chunk (Bytes.empty ()) s
  if Bytes.length buf == 0 then Net.close s
  else
    let (_, s) = Net.write_bytes buf s
    serve_fresh s

serve_reuse : Bytes -> Net.TcpStream -> <IO, Panic, Async> Unit
serve_reuse b s =
  let (b, s) = Net.read_chunk (Bytes.truncate 0 b) s
  if Bytes.length b == 0 then Net.close s
  else
    let (b, s) = Net.write_bytes b s
    serve_reuse b s

rounds_fresh : Int -> Int -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
rounds_fresh n size s =
  if n == 0 then s
  else
    let (_, s) = Net.write_bytes (Bytes.make size 'x'u8) s
    let (_, s) = read_exact size (Bytes.empty ()) s
    rounds_fresh (n - 1) size s

rounds_reuse : Int -> Int -> Bytes -> Bytes -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
rounds_reuse n size msg b s =
  if n == 0 then s
  else
    let (msg, s) = Net.write_bytes msg s
    let (b, s) = read_exact size (Bytes.truncate 0 b) s
    rounds_reuse (n - 1) size msg b s
```

ファイルの頭のコメントの mode の2行を上の2行に替え、4つの関数を上の形に替える。ほかの行は変えない。

`bench/net/bulk.em`:

```haskell
--   wmode: fresh なら書くたびに Bytes.make で新しい buffer を作る、shared なら同じ buffer を何度も渡す (共有のまま貸す)

send_fresh : Int -> Int -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
send_fresh n size s =
  if n == 0 then s
  else
    let (_, s) = Net.write_bytes (Bytes.make size 'x'u8) s
    send_fresh (n - 1) size s

send_shared : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
send_shared n b s =
  if n == 0 then s
  else
    let (_, s) = Net.write_bytes b s
    send_shared (n - 1) b s
```

`bench/net/many.em`:

```haskell
answer : Net.TcpStream -> <IO, Panic, Async> Unit
answer s =
  let (req, s) = Net.read_chunk (Bytes.empty ()) s
  let (_, s) = Net.write_bytes req s
  let (_, s) = Net.read_chunk (Bytes.empty ()) s
  Net.close s

ask : Net.TcpStream -> <IO, Panic, Async> Unit
ask c =
  let (_, c) = Net.write_bytes (Bytes.make 16 'q'u8) c
  let (_, c) = Net.read_chunk (Bytes.empty ()) c
  Net.close c
```

`conn.em` と `base.em` は `write_bytes` を使わないので変えない。この差分は試していない (新しい型がまだない)。形は同じファイルの `let (_, s) = read_exact …` と同じである。

### 8.4 T-9 が足す `busy.em` と `busy-self` の行

`busy-self` は、計算の重いタスクが走る worker で、集める地点が OS を見る頻度 (本体 D-7。`POLL_EVERY`) が往復の時間に出るかを見る件である。同じ run の中で、16 バイトの echo を `n` 往復する2つのタスク (クライアントとサーバー。echo.em の reuse の形) と、計算だけを続けるタスク1つを、CLI の既定の `--sched normal` で動かす。計算のタスクは待ちの操作をせず、横取りの刻みでだけ切り替わる。

下は T-8 の後の API で書いた形である。同じ形を基点の API (`write_bytes` が `TcpStream` を返す) に直したものは、基点の release の eml で組めて流れた。`busy.em 50` が 2.1 s (1往復約 40 ms) かかった [V]。1往復に横取りの刻みを何回か待つためと推定する。そのため、本体 T-9 の条件に従って往復の数を決める。

```haskell
-- (e) 計算と I/O が混ざる run。同じ run の中の 16 バイトの echo を `n` 往復する2つのタスクと、計算だけを続けるタスク
-- 1つを動かす。計算のタスクは待ちの操作をせず、横取りの刻みでだけ切り替わる。I/O の完了を拾う集める地点の頻度
-- (docs/spec/vm.md の「時計と driver」) が往復の時間に出るかを見る
-- 引数: n
import Async (Async)
import Async
import Atomic
import Bytes
import Env
import Net
import String

serve : Bytes -> Net.TcpStream -> <IO, Panic, Async> Unit
serve b s =
  let (b, s) = Net.read_chunk (Bytes.truncate 0 b) s
  if Bytes.length b == 0 then Net.close s
  else
    let (b, s) = Net.write_bytes b s
    serve b s

-- `want` バイト集まるまで読む
read_exact : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> (Bytes, Net.TcpStream)
read_exact want b s =
  if Bytes.length b >= want then (b, s)
  else
    let (b, s) = Net.read_chunk b s
    read_exact want b s

rounds : Int -> Bytes -> Bytes -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
rounds n msg b s =
  if n == 0 then s
  else
    let (msg, s) = Net.write_bytes msg s
    let (b, s) = read_exact 16 (Bytes.truncate 0 b) s
    rounds (n - 1) msg b s

fib : Int -> <Panic> Int
fib k = if k < 2 then k else fib (k - 1) + fib (k - 2)

-- 旗が立つまで計算を続け、回った数を返す
spin : Int -> Atomic.Counter -> <IO, Panic> Int
spin done flag =
  if Atomic.load flag != 0 then done
  else
    let _ = fib 15
    spin (done + 1) flag

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let n = int_arg 0 2000
  let flag = Atomic.new 0
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    serve (Bytes.empty ()) (Net.set_nodelay True s))
  let busy = Async.start (fn () -> spin 0 flag)
  let c = Net.set_nodelay True (Net.connect "127.0.0.1" port)
  Net.close (rounds n (Bytes.make 16 'x'u8) (Bytes.empty ()) c)
  Async.await server
  Atomic.store 1 flag
  let _ = Async.await busy
  println "busy done \{show n}")

-- 引数の読み取り。各プログラムが1ファイルで動くように、同じ定義を写して持つ
int_arg : Int -> Int -> <IO, Panic> Int
int_arg i default =
  match nth i (Env.args ()) with
    | Some s ->
      match String.to_int s with
        | Some n -> n
        | None -> default
    | None -> default

nth : Int -> List String -> <Panic> Option String
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else nth (i - 1) rest
```

`cases.tsv` の行 (区切りはタブ。`conn-py` の行の後、`py:echo` の行の前に置く):

```
busy-self	busy.em	2000	200	-	-	2000	200	往復
```

往復の数 2000 と 200 は既定である。本体 T-9 で `busy.em 200` の1回の実時間を測り、10 s を超えるなら、時間の引数を1回の run が 1〜3 s に収まる数に、小さい引数をその 1/10 に下げ、単位の数の2列も同じ値にする。下げた数は計画書とコミットのメッセージに書く。

## 9. 確かめていないこと

- strace の下では時間が伸びるので、poll が空振りする回数と `brk` の回数が、strace なしの run と違いうる (echo-self-fresh で `brk` が 0 か 2 になった)。strace なしで syscall を数える道具 (perf、eBPF) はこの container にない。
- 確保の揺れが何で決まるか (バイナリの配置、確保と手放しの順) は突き止めていない。`GLIBC_TUNABLES=glibc.malloc.trim_threshold=33554432` でも揺れは消えなかった。
- many-self の RSS が本数とともに段になる理由は推論で、素の run で同時に生きた buffer の数は数えていない。4000 本の callgrind は取っていない。
- 確保の数は検査モードの値だけで、Rust の側の確保 (compio の `Key`、`HashMap` の作り直し) は数えていない。VM のタスクの切り替えの数は、数える欄がないので `parks` から推した。
- 送りの buffer があふれる書き (書けるようになるまで待つ道) と、`Read` の受け付けが EAGAIN になる道は、基点の件では起きず、測っていない。
- macOS と Windows では流していない。`run.py` の macOS の分岐と `peer.py` が macOS で動くかは確かめていない。
- `-py` の件では Python の相手が律速する。実時間は eml の片側の遅れの上限としてだけ読める。
