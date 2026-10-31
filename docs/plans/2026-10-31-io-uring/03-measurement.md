> 設計の段階の付録 (英語)。優先の順は、本文 (`../2026-10-31-io-uring.md`) > `05-rulings.md` > この付録である。`scratch/…` や `scratch-s*/…` の名前は設計のときの作業用のファイルで、リポジトリにはない。本文に要るものは本文か `06-harness.patch` に写してある。

# S2 (r4): measuring the io_uring prototype, judging the gate, and what the docs say in each outcome

Tags: [V] verified (how is given), [I] inferred.

r4 applies the binding conformance resolutions (X-resolutions-r1: Cf-1, Cf-3, Cf-6, Cf-8, Cf-10, Cf-11) and the amended G-11 (X-ruling-r1 §4, approved by the user). Everything else stands from r3: F1 r2 policy, the user answers, S1's arms.

Scratch evidence lives in `scratch-s2/`:
- `harness-r2.patch` (the M1 diff), `check.py`
- `probe/` (the rusage probe)
- JSONL files `aa1`, `aa-full`, `chk`, `g3`, `eu`

## 1. Conclusion

**Commit order.**
1. BASE: the stage's plan commit. It leaves roadmap line 31 as it is.
2. M1: the harness, plus the `--env` sentence in benchmarks.md.
3. P1 and P2 (S1).
4. M2: the `io_uring_enter` column.
5. M3: the gate result, recorded in the plan.
6. Then S3: R0, and `Record the io-uring decision` (plan only), or D1 if the gate failed. In the continue branch, S3's units and Close follow.

**Harness (M1).**
- `bench.py` gets `--env LABEL KEY=VALUE`, repeatable. Each JSONL line gets an `"env"` field.
- `report.py` compares the first label with each later label, one table per pair, with these columns:
  - `比` to 3 decimals
  - `生の CPU の比`
  - a raw `base` row
  - `minflt/単位の差`
  - `確保の揺れ`, which holds `印`. It is recorded only and never vetoes.
- M1 also adds one sentence describing `--env` next to the `bench.py` usage paragraph in benchmarks.md 「ネットワークの計測」 (Cf-3).

**Arms.** The epoll arm comes first:

```sh
ARMS="epoll=$S/eml-proto uring=$S/eml-proto direct=$S/eml-proto --env epoll EML_IO_BACKEND=epoll --env uring EML_IO_BACKEND=io_uring --env direct EML_IO_BACKEND=io_uring_direct"
```

- Label names are free. `check.py` reads each label's `env`, not its name. I checked this with S3's labels `e` and `u` (Cf-11).

**Gate cases.**
```sh
G='^(base|many-self-1000|echo-self-fresh|echo-py-fresh|echo-py-reuse|bulk-1k)$'
```

**Gate rule.**
- A session passes (U, c) when, in 「epoll → U」, `比 ≤ 0.800` and `判定` is 「差」. U is a ring arm and c a gate case.
- An echo-py row counts only when U's regime line is `ok`.
- Sessions 1 and 2 must both pass. If they disagree, session 3 decides.
- The gate passes when any (U, c) passes, the A/A shows 差なし on all 5 cases, and every `backend` line is `ok`.
- The arm that passed is the one shipped. If both arms pass, the arm with more passing cases wins. If that ties, the arm with the lower mean 比 over the 5 cases (sessions 1 and 2) wins.

**What M3 records (Cf-8).**
- Gate passed: `門: 通った <腕>`.
- Gate failed: `門: 通らない` and `判断: 捨てる`, then D1.
- When the gate passes, `判断` is not written in M3. S3 writes it after R0, in `Record the io-uring decision`.

**rusage.** io-wq and `DEFER_TASKRUN` work is billed to the eml process [V probe: rusage / system busy 0.95–0.99]. M3 must run on `6.18.44-fc-v185`.

**Discard.**
- D1 reverts every code commit after BASE. Afterwards, `git diff $BASE --` over the shared path list (Cf-10) is empty.
- D2 deletes roadmap line 31 and writes the 「その後の項目」 item (Cf-6). It adds `### io_uring と epoll` and a regime bullet to benchmarks.md, and a pointer to async.md.
- D2's completion includes the amended G-11 discard check.

**Continue.**
- A failure of the Close gate follows S3 §1.8: fix and rerun; if that cannot be fixed, discard through §1.8, which runs D1 and D2 (Cf-1).
- At Close, S3 pins the older recorded commands with `EML_IO_BACKEND=epoll` (Cf-3).

## 2. Rationale

Unchanged from r3. In brief:
- Every noise condition is hygiene that cannot fail a true 20% drop on an idle machine.
  - IQR widths: 0.004–0.040 s, against shifts of 0.016–0.11 s [V].
  - The 2-of-3 rule passes with probability 3p²−2p³ ≥ p when p ≥ 0.5.
  - The regime check binds only when the ring arm itself shares a CPU.
  - 確保の揺れ is recorded, not a veto.
- One binary for all arms isolates the backend.
- The A/A ratios were 0.932–1.084 [V g3].

Reasons for the r4 changes:
- **Cf-1.** The user's binary rule (fix or discard) leaves no third outcome at Close, so the Close failure path is S3 §1.8 and does not return to the user.
- **Cf-3.** M1 introduces `--env`, so M1 owns the sentence that describes it. Discard reverts M1, and the sentence goes with it. Pinning the older commands only makes sense once a second backend ships.
- **Cf-8.** R0 can still turn a passed gate into a discard, so committing `続ける` in M3 could contradict a later commit. A failed gate cannot turn into a continue, so M3 records `捨てる` directly in that case.

## 3. Alternatives considered and rejected

As in r3. Added in r4:
- **M3 records `判断: 続ける`.** R0 can reverse that (Cf-8).
- **Rewriting line 31 in place at BASE.** Rejected by Cf-6.

## 4. Assumptions made

- S1's switch:
  - unset or `epoll` → epoll; `io_uring` → try-first; `io_uring_direct` → ready-gated; anything else fails.
  - The ring run calls `io_uring_setup` once on `main` and makes no `epoll_create1` [I, S1 P2 bar].
- `base` builds the driver [V].
- The BASE eml ignores the variable [V: in `eu.jsonl` the `io_uring`-pinned label ran epoll].
- Merges are merge commits.
- S3 U4 commits `bench/net/check.py` from `scratch-s2/check.py`. The plan carries that text verbatim, as the single source (Cf-11). M3, which runs before U4, uses the copy at `$S/check.py`.

## 5. Implications for implementation

### M1 `Pass per-label environment to bench/net`

- **`bench.py`.**
  - `main()` parses `--env`. On an unknown label or a missing `=`, it exits with `bench.py: --env {label} {kv}: ラベルがないか KEY=VALUE でない`.
  - `run(extra, cmd, env)` calls `subprocess.run(..., env={**os.environ, **env})`.
  - `command()` unpacks `(eml, progs, _)`.
  - `emit(..., env)` writes the env into each line.
  - `has_stats` gets the label's env.
  - The docstring gains the option.
- **`report.py`.**
  - `for b in labels[1:]`, with the header `| 件 | A | B | 比 | 生の CPU の比 | 四分位の重なり | 判定 | minflt/単位の差 | 確保の揺れ | 実時間の比 |`.
  - The `base` row is raw and never gets `印`.
- **`benchmarks.md` 「ネットワークの計測」.**
  - In the `bench.py` usage paragraph (line 385 today), add the `--env` sentence. It says that each label's runs (time, strace, stats) get the variable, and that each JSONL line records it in `env`.
  - In the 確保の揺れ bullet of 「比べる規則」, add that the table marks it.
  - Write the Japanese with natural-japanese.
- **Completion.**
  1. The `--env` round trip prints `4 ['{"EML_IO_BACKEND": "epoll"}', '{}']`.
  2. A bad `--env` exits with status 1.
  3. Three labels give two `## 比べ:` tables.
  4. `rg -n -- '--env' docs/implementation/benchmarks.md` prints the new sentence.

### M2

- Append `"io_uring_enter"` to `SYS`.
- Completion: every ring label's value in that column is above 0.

### M3 `Record the io-uring gate` (plan only)

Preconditions, checked before each session:
- `uname -r` = `6.18.44-fc-v185`
- `git status --short` is empty
- no other builds or agents are running
- the first field of `/proc/loadavg` is below 0.5

```sh
S=$(mktemp -d); BASE=<the plan commit, the parent of M1>
mkdir "$S/base" && git archive "$BASE" | tar -x -C "$S/base"
cargo build --release -p eml_cli --manifest-path "$S/base/Cargo.toml" && cp "$S/base/target/release/eml" "$S/eml-main"
cargo build --release -p eml_cli && cp target/release/eml "$S/eml-proto"
G='^(base|many-self-1000|echo-self-fresh|echo-py-fresh|echo-py-reuse|bulk-1k)$'
ARMS="epoll=$S/eml-proto uring=$S/eml-proto direct=$S/eml-proto --env epoll EML_IO_BACKEND=epoll --env uring EML_IO_BACKEND=io_uring --env direct EML_IO_BACKEND=io_uring_direct"
python3 -I bench/net/bench.py --only "$G" --runs 11 --strace-runs 3 main=$S/eml-main epoll=$S/eml-proto --env epoll EML_IO_BACKEND=epoll > $S/aa.jsonl
python3 -I bench/net/bench.py --only "$G" --runs 11 --strace-runs 3 $ARMS > $S/gate1.jsonl
python3 -I bench/net/bench.py --only '^(?!busy-self$)' --runs 11 --stats $ARMS main=$S/eml-main > $S/full.jsonl
python3 -I bench/net/bench.py --only "$G" --runs 11 --strace-runs 3 $ARMS > $S/gate2.jsonl
for f in aa gate1 full gate2; do python3 -I bench/net/report.py $S/$f.jsonl > $S/$f.md; python3 -I $S/check.py $S/$f.jsonl > $S/$f.check; echo "$f $?"; done
```

**`check.py`.** This is the single source. The plan carries it verbatim, and S3 U4 commits it unchanged.

```python
import json, statistics as st, sys
rows = [json.loads(l) for l in open(sys.argv[1])]
get = {(r["case"], r["label"], r["kind"]): r for r in rows}
labels = list(dict.fromkeys(r["label"] for r in rows if r["label"] != "python"))
uring = lambda r: r["env"].get("EML_IO_BACKEND", "epoll") != "epoll"
bad = 0
for r in rows:   # 1) 各ラベルが名前どおりの backend で動いたか
    if r["kind"] == "strace":
        for d in r["data"]:
            m = d["syscalls"]["main"]; u, e = m.get("io_uring_setup", 0), m.get("epoll_create1", 0)
            ok = (u >= 1 and e == 0) if uring(r) else (u == 0 and e >= 1)
            bad += not ok; print("backend", r["case"], r["label"], u, e, "ok" if ok else "NG")
for l in labels:   # 2) echo-py の切り替えの形
    for c in ("echo-py-fresh", "echo-py-reuse"):
        r = get.get((c, l, "time"))
        if not r:
            continue
        v, i = st.median(x["nvcsw"] for x in r["data"]), st.median(x["nivcsw"] for x in r["data"])
        ok = i <= 1000 and (uring(r) or v >= 15000)
        bad += not ok; print("regime", c, l, int(v), int(i), "ok" if ok else "NG")
sys.exit(bad)
```

- The script never names a label.
- An arm counts as a ring arm when its env value is anything other than `epoll`. A label with no env (`main`) is epoll.

**Expected results.** L is the number of labels.

| File | L | `wc -l` | check exit | check lines | report |
|---|---|---|---|---|---|
| aa | 2 | 24 [I] | 0 | 36 `backend … ok`, 4 `regime … ok` | 「main → epoll」: 5 gate rows 差なし |
| gateK | 3 | 36 [V g3] | 0 | 54 `backend`, 6 `regime` [V g3] | 「epoll → uring」, 「epoll → direct」 |
| full | 4 | 243 [I] | 0 | 240 `backend`, 8 `regime` | plus 「epoll → main」, all 差なし expected |

**Failure handling** (as in r3):
- A ring arm's `regime` NG voids only that arm's echo-py rows; record them as 「切り替えの形が違う」.
- An epoll or main `regime` NG voids the session: rerun it after the idle check, at most twice, then report.
- Any `backend` NG voids everything.
- If the A/A shows 差:
  1. Rerun it once.
  2. If it still shows 差, run a same-binary A/A.
  3. Then: S1 fixes it (比 ≥ 1.10), or proceed (比 ≤ 0.90), or stop and report.
- Session 3: `--only '^(base|<case>)$'`, same `$ARMS`.

**Recording.** The plan's 「計測の結果」 gets:
- for each arm and case: 比 and 判定 for sessions 1, 2 and 3, and the raw ratio
- the A/A range
- the check status
- the regression list for each arm (判定 「差」 with 比 ≥ 1.100, `base` included), handed to S3 §1.7
- the full 比べ tables
- one line: `門: 通った <腕>`, or `門: 通らない` followed by `判断: 捨てる`

The commit message (English) carries the same lines, plus the machine, BASE and `<P2>`.

When the gate passes, M3 writes no `判断`. S3 records it after R0 in `Record the io-uring decision`.

### Discard

There are four ways in, all ending in D1 then D2:
- (a) M3's gate fails.
- (b) R0 or the CI shows that the hosted runner refuses io_uring.
- (c) A blocking regression cannot be fixed.
- (d) The Close gate fails and fix-and-retry did not succeed (S3 §1.8).

**D1 `Remove the io_uring prototype`.**
- Revert every code commit after BASE, newest first: S3's units if any, in S3's order, then M2, P2, P1, M1.
- Completion:
  - `git diff $BASE -- crates bench .config .github flake.nix Cargo.toml Cargo.lock THIRD-PARTY-LICENSES.md docs/spec README.md CLAUDE.md docs/implementation/testing.md` is empty. This is the same list as S3 §1.8 (Cf-10).
  - `cargo nextest run` passes.
  - `cargo clippy --all-targets -- -D warnings` is clean.

**D2 `Close io-uring`.** Follow the roadmap 「進め方」 and use natural-japanese.

- **benchmarks.md `### io_uring と epoll`.** Place it after 「書きの重みの単位」, before 「## 所見」. Contents as in r3:
  - Opening: names `<P2>`, the three env values, the one-binary method and the machine.
  - Command block: the gate command with all three `--env` pins, as the record of the tree at `<P2>`. No line in this section may start with `#`, including inside the code block, because the awk check below treats such a line as a heading.
  - Rule sentence: names the five cases. It states that the 確保の揺れ mark was recorded, not used as a veto.
  - Gate table for each arm, for runs 1 and 2. Then the full table and the bullets.
  - The discard reason, one of these variants:
    - (a) no case reached 0.800
    - (b) the gate passed with `<腕>` on `<件>`, but the hosted runner could not build io_uring
    - (c) the gate passed, but a regression on `<件>` at `<比>` could not be fixed
    - (d) the gate passed on the prototype, but the final binary did not pass (`<腕>`, `<件>`, `<比>`)
  - Last bullet: 「Linux の backend は epoll である ([ロードマップ](../future/roadmap.md) の「その後の項目」の「Linux の io_uring の backend」)」.
  - Wording limits: no stage name, no date, no 「今回」 or 「捨てた」.
- **benchmarks.md 「比べる規則」.** Add the regime bullet: echo-py-fresh 4.67 µs (650/19374) under load against 28.04 µs (19378/37) idle, on a2dafe7. M1's `--env` sentence is gone with D1.
- **roadmap.md (Cf-6).**
  - Delete the `io-uring` entry from 「段の列」, in whatever text it has; BASE leaves it unchanged.
  - Write 「Linux の io_uring の backend」 under 「その後の項目」 → 「### 非同期」. The body:
    - states the policy: epoll is Linux's reference backend, and io_uring would enter only as the default with an epoll fallback;
    - points to the prototype commit `<P2>`, naming it as the tree where `EML_IO_BACKEND` chose the backend. That puts the one `EML_IO_BACKEND` mention the G-11 check expects in roadmap.md;
    - points to [計測の基準](../implementation/benchmarks.md) の「io_uring と epoll」.
  - The body carries none of these: 5.11 / `EXT_ARG`, `user_data`, teardown, "CI on both".
  - **後に回した理由**, one variant:
    - (a) 「試作では門の5件のどれでも CPU が2割縮まなかった (いちばん縮んだのは <腕> の <件> の <比>)。貸した buffer を1つだけカーネルに渡す契約の下では、io_uring の利点は待ちの syscall の削減に限られる。」
    - (b) 「門は <腕> の <件> (<比>) で通ったが、hosted runner が io_uring を作れず、CI で確かめられない backend を既定にしない。」
    - (c) 「門は <腕> の <件> (<比>) で通ったが、<件> の <比> の後退を直せなかった。」
    - (d) 「試作では門を通ったが、仕上げた backend では <件> が <比> で、2割に届かなかった。」
  - **見直す条件** (F1 §5.2):
    1. ネイティブ化の後に many-self-1000 と echo-self-fresh を測り直し、記録した syscall の削減分が CPU の2割に当たるとき
    2. buffer の契約を変えて、カーネルが選ぶ buffer を受け入れる設計をするとき
    3. `async-files` で、プールの仕事の費用が計測で目立ったとき
    4. variant (b) only: runner が許したとき
    - variant (c) adds: 後退の原因を除く設計ができたとき
- **async.md:243.** After 「io_uring は使わない」, add 「(理由は [計測の基準](../implementation/benchmarks.md) の「io_uring と epoll」)」.
- **Completion.**
  - The roadmap entry is gone: `grep -n '^- .io-uring.:' docs/future/roadmap.md` prints nothing (it prints line 31 today).
  - The new item exists: `awk '/^### 非同期/{f=1;next} /^### /{f=0} f && /Linux の io_uring の backend/' docs/future/roadmap.md` prints exactly one line.
  - The amended G-11 discard check (X-ruling-r1 §4):
    - `rg -l -- '--env|EML_IO_BACKEND' bench docs` prints exactly `docs/implementation/benchmarks.md` and `docs/future/roadmap.md`. Today it prints only `docs/future/roadmap.md` [V].
    - `awk '/^### io_uring と epoll/{f=1;next} /^#/{f=0} /--env|EML_IO_BACKEND/ && !f {print NR}' docs/implementation/benchmarks.md` prints nothing.
    - `rg -n -- '--env' bench` prints nothing.
  - `grep -n 'io_uring と epoll' docs/future/async.md` prints the pointer line.
  - `ls docs/plans` shows nothing from this stage.
  - The D1 diff stays empty.
- **Close message.**
  - what moved where
  - the gate rows
  - the variant and the date it was decided
  - the macOS 「今の数値」 waiver (the user waived it; the tree equals BASE on the D1 path list)

### Continue (measurement side; S3 owns the rest)

- M1 and M2 stay. S3 U4 commits `bench/net/check.py` (the text above) and splits the target tables.
- **The Close rerun.** The same commands, run on the final binary, with only the shipped arm's label (e.g. `uring=… --env uring EML_IO_BACKEND=io_uring`). Same gate rule. On failure, S3 §1.8 governs: fix with new commits and rerun; if that cannot be fixed, discard through §1.8, which ends in D1 and D2 variant (d) (Cf-1).
- **`### io_uring と epoll`** is written at Close from the Close numbers, in the same layout. Its rule sentence says io_uring was made the default. Its last bullet cites vm.md (S3). The prototype numbers go only into the M3 and Close commit messages.
- **G-11, continue check (owned by S3 at Close, Cf-3).** This needs `rg -n 'bench.py' docs | rg -v -- '--env' | rg -v 'main=|BASE|<BASE>'` to print nothing.
  - Line 385 already carries M1's sentence.
  - Today's other hits are benchmarks.md 432, 505, 523 and 524 [V on a2dafe7]. S3 pins them with `--env <label> EML_IO_BACKEND=epoll`, using the labels those sections name (`one`/`sixteen`, `u512`/`u256`/`u1024`), and adds a sentence saying these values are epoll's.
  - The pinned io_uring target table gets its own pinned command (S3 U4).
- **「driver の syscall の目標」.** The epoll table stays the reference (G-1).

## 6. Unresolved items (with defaults)

1. **The idle thresholds.**
   - Load average below 0.5.
   - echo-py on the epoll side: voluntary switches ≥ 15000, involuntary ≤ 1000.
   - Default: use these. If a session fails twice on an idle machine, report rather than relax them.
2. **SQPOLL billing is untested.** SQPOLL is excluded by F1.
3. **Squash merge.** Default: merge commit. If the stage is squash-merged, cite main's equivalent commit for `<P2>`.

## 7. Additional checks

- [V] check.py with S3's label names (`eu.jsonl`, a2dafe7 eml, labels `e` and `u`, with `u` pinned to `io_uring`, which that eml ignores):
  - It printed `backend base u 0 1 NG` and `backend echo-py-fresh u 0 1 NG`, so a ring-pinned label that ran epoll is detected.
  - It printed `regime echo-py-fresh e … ok` and `regime echo-py-fresh u … ok`.
  - It exited 2.
  - `report.py` printed `## 比べ: e → u`.
- [V] `rg -l -- '--env|EML_IO_BACKEND' bench docs` on a2dafe7 prints only `docs/future/roadmap.md` (from line 31). Roadmap line 31 contains `EML_IO_BACKEND`.
- [V] From r3, still valid: the 3-label gate session (136 s, 36 lines, 54 backend ok, 6 regime ok); the rusage probe; the 5 hits of the G-11 grep.
- No background tasks are running.

