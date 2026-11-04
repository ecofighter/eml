# async-files: commit-by-commit implementation design

Tags: [V] verified (how is given), [I] inferred. Paths are relative to the repository root. "S1" = `design.md` in this directory (the guideline; G-n are its conformance criteria, G-7 as amended by `ruling.md`). Commit numbers here are the plan's tasks T-1..T-13 (`../2026-11-04-async-files.md` §7); the plan document itself is the first commit and is not listed.

## 1. Conclusion

### Decisions this design adds below S1 (the implementer does not re-decide these)

| ID | Decision | Why |
|---|---|---|
| D1 | Structural launch errors (redirected stdout on a non-last stage, `PipeStderr` outside `Exec.spawn`'s last stage) are raised in eml by `Proc.launch_args` with `fail_io Other` and the exact S1 texts, naming the offending stage's program. Host-side launch errors carry `io::Error` payload `LaunchError { stage: usize, cause: LaunchCause }`; `launch` maps `LaunchCause::Spawn` to `Fault::Io { op: Run, name: <stage program> }` and `StdinText`/`Open(path)` to a new `Fault::Launch { program, cause, reason }`; an error without payload maps to `Fault::Io { op: Run, name: <last program> }`. | `Fault::Io` renders only ``cannot run `p`: <reason word>`` ([V] object-model.md:1014, externs.rs:194-213) and cannot name a file; S1 §5's stage encoding has no stdout field for middle stages. The payload fixes how the failing stage index reaches `launch` (review A-1). Kinds follow S1 A4 ("with the open's kind"): `Other` for structural errors, the reason's kind for `Launch`. |
| D2 | `Proc.launch_args : Int -> Int -> Bool -> Cmd -> <IO> (String, Int, String, Int, String)` is `pub`, documented as "`Std.Exec` のためのインターフェース", and is the only walker/validator/encoder of a `Cmd`. Its comment must not contain the word `Async` (G-1 grep). | Non-`pub` std items are invisible to other std modules ([V] modules.md:85); precedent: `io_wait` is `pub` as "`Std.Net` のためのインターフェース" ([V] stdlib.md:1046). |
| D3 | Files are bound at open: `Stream.open/create/append` submit `Op::Adopt { res: RawResource::File }`, answered at submit with `Done::Handle(IoHandle)` by the driver of the opener's I/O host (`Run::io_worker(k)`). The opener parks only when that host is another worker (virtual clock, k >= 1: `Submit`/`Completion` mail round trip), and the record then settles like `connect`'s. Pipe ends stay `Bind::Unbound` and are adopted lazily by `bind_stream`. | Approved amendment of G-7 (ruling.md Q1). |
| D4 | One Rust payload `IoStream { bind, origin: Origin, rest, start }` replaces `NetStream` and backs `Net.TcpStream`, `Stream.Reader`, `Stream.Writer`; `Origin = Peer(SocketAddr) | File(Arc<str>)` (commit 7), `| Child { program: Arc<str>, pipe: ChildPipe }` added in commit 11 with its first constructor. Distinct eml types and type rows. | All record helpers read `foreign_unique::<NetStream>` ([V] cells.rs:4167, 4300, 4319, 4344, 4405). |
| D5 | Outcome codes 0-6 unchanged, `outcome()` and `io_kind` unchanged ([V] cells.rs:530-549, vm.rs:3207-3214); `BrokenPipe` is code 2. Records carry a `Prefix` chosen at submit from `Origin` (Net keeps its literal texts); `Taken::failure()` builds code-6 messages `"<prefix>: <io::Error>"` ([V] cells.rs:500-507). New borrowing row `op_prefix` per module (`Std.Stream.op_prefix`, `Std.Exec.op_prefix`, one shared VM arm); wrappers follow Net's branch with `let (prefix, op) = op_prefix op` before `finish`, and a module-local `fail_code` (code 2 -> `ConnectionReset "\{prefix}: broken pipe"`, 3-5 as Net.em:248-251). No `op_failed`. | ruling.md Q2 (required change). |
| D6 | Every `Std.Stream` and `Std.Exec` row is in `Extern::is_async()` and dispatched in `vm.rs`'s `Async` arm, including the `<IO>` rows that S1 §5's Constraints bullet placed in the generic extern arm (`open_raw`'s row `submit_open`, the takers `stdin`/`stdout`/`stderr`, `reap`); that bullet does not apply. | They read or make `eml_vm` payloads (`IoStream`, `ExecChild`) that `Ctx` cannot see, as `Net.local_port`/`peer_addr` do ([V] lib.rs:1706-1707). |
| D7 | `CaptureHost` keeps one store of written files: a `BTreeMap<String, usize>` from the path as written to a flat file `<dir>/<n>` in a temp directory created on the first write and owned by `Written`'s `Arc`; removed (errors ignored) when the last `Written` clone drops. | Redirections and `open_raw` need real fds; one store keeps cross-API reads consistent; `ui.rs` reads `written` after `execute` drops the host ([V] ui.rs:186-213). |
| D8 | `Proc.wait` (extern) closes stdin, reaps every stage head first, returns the last stage's code. `Proc.wait_all` is eml over `stage_count` and `wait_stage`. | Keeps `Proc.wait : Child -> <IO> Int`. |
| D9 | Launch errors (`Fault::Launch`, `Fault::Io { op: Run }`) keep the existing `Proc` style of the four fixed reason words; code-6 read, write and wait messages of `Stream`/`Exec` keep the OS text as `Net` does. | Resolved in consolidation; stdlib.md 「`Proc`」 失敗 table already uses the words; `Fault` has no OS text ([V] externs.rs:200). |
| D10 | `SPAWN_LOCK` is a leaf lock (rule in section 5 "SPAWN_LOCK"). | Review B-6. |
| D11 | `open_raw` refuses any directory with `NOT_REGULAR` on every OS and mode: an open that fails while `fs::metadata(path)` is a directory, and an open that succeeds on a non-regular file, both return `io::Error::new(Unsupported, NOT_REGULAR)`. So `Stream.open/create/append` of a directory give ``NotAvailable: cannot open `<path>`: not a regular file`` on unix and Windows; the UI test carries no Windows mark. `FileIn.open`'s `IsADirectory` behaviour is unchanged. | Windows fails `File::open` on a directory and `directory_error` turns it into `IsADirectory` ([V] host.rs:1416-1425); unix create on a directory fails `EISDIR` (review B-5). |

### Commit sequence

| # | Title | Depends on | G-n |
|---|---|---|---|
| 1 | Keep CaptureHost's written files in a temp directory | – | G-9 (store) |
| 2 | Pass a stage list and stdio specs to Host::spawn | – | – (refactor) |
| 3 | Add stdin and stdout modes to Cmd | 1, 2 | G-3, G-11 (part) |
| 4 | Spawn a Cmd pipeline in one Host call | 3 | G-2, G-11 |
| 5 | Run regular-file reads and writes as pool jobs | 4 | G-5, G-7 (files, driver side), G-10 |
| 6 | Model regular files in the test driver | 5 | G-8 |
| 7 | Add Std.Stream | 6 | G-1, G-5, G-6, G-7 (files), G-9, G-12 |
| 8 | Add pipes and child-exit waits to the epoll and kqueue backends | 5 | G-2 (pipe lock), G-4, G-7 |
| 9 | Add pipes and child-exit waits to the IOCP backend | 8 | G-4, G-7 |
| 10 | Add the real-clock mark to UI tests | – | G-8 |
| 11 | Add Std.Exec | 7, 9, 10 | G-1, G-4, G-7, G-8, G-11, G-12 |
| 12 | Add the standard error pipe to Exec | 11 | G-11 |
| 13 | Close async-files | 12 | all |

Every commit must end with, on its own tree: `cargo nextest run` passes; `cargo clippy --all-targets -- -D warnings && cargo fmt --check` clean; the feature-combo clippy lines of CLAUDE.md clean; `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings` clean (and the two Linux targets); `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` passes. No non-`pub` item, variant or field is introduced before the commit that reads or constructs it; `pub` items of `eml_rt` introduced with their implementing code are listed per commit. Commits touching `crates/eml_rt` add the gate of section 5 Q4.

---

### Commit 1: Keep CaptureHost's written files in a temp directory

Purpose: D7, so later commits can hand real fds of written files to children and to the driver.

Depends on: S1 A7 ("CaptureHost::open_raw … per-run temp files", G-9).

Files: `crates/eml_vm/src/hosts.rs`, `docs/implementation/testing.md`.

Concrete change:
- `Written(Arc<Store>)`, `struct Store { files: Mutex<BTreeMap<String, usize>>, dir: OnceLock<PathBuf>, next: AtomicUsize }`; `impl Drop for Store` removes `dir` with `fs::remove_dir_all` (errors ignored). `dir` is created on the first write: `std::env::temp_dir()/eml-written-<pid>-<n>` with `create_dir` (retry on `AlreadyExists`).
- `files()` reads every entry's file in path order (same order as today's `BTreeMap` [V] hosts.rs:42-56).
- `CaptureHost::open_write(path, mode)`: `Truncate` maps `path` to a fresh `<n>` (or truncates the existing one), `Append` opens the existing one with `append(true)` (or creates it); returns the `File`. Any path string works, including ones with `/` and missing parents (flat names), as today.
- `CaptureHost::open(path)`: path in the store -> `Opened { reader: File, size: Some(len) }`; else `inner.open` as today.
- Deletion point (G-9 "deleted on drop" read as the drop of the last `Written` clone, not of the host): `ui.rs` reads `written` after `execute` has dropped the host ([V] ui.rs:186-213), so the files must outlive the host until that read.
- Leaks: a run ended by `process::exit`, an aborting panic, or a killed test process leaves `eml-written-*` in the temp directory; nothing is ever written under the repository.

Spec: testing.md 「UI テスト」: written files go to a temp directory of the test Host, made on the first write and removed when the run's view of the files drops; they never go into the repository (an aborted test process can leave the temp directory behind).

Tests:
- New unit tests in `crates/eml_vm/src/hosts.rs` (existing `#[cfg(test)]` style): `written_files_keep_the_path_order`, `a_path_with_missing_directories_is_written`, `a_truncate_after_an_append_starts_empty`, `a_written_file_is_read_back_by_open`, `the_directory_is_made_on_the_first_write_only` (a host that writes nothing creates no directory), `the_directory_is_removed_when_the_last_view_drops`.
- Test-change kind: no existing test file or expected value changes; every UI snapshot with `--- written:` and every REPL transcript must stay byte-identical (this is a behavioural rewrite of the test Host, checked by the unchanged snapshots).

Verification:
- `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::)'` -> all pass with no snapshot changes (`cargo insta pending-snapshots` lists nothing).
- `cargo nextest run -p eml_vm --lib -E 'test(hosts::)'` -> the six new tests pass.
- `git status --porcelain -- tests/` prints nothing after the run.
- `cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)' -j 1` total time not more than 0.2 s above the time on the parent commit (measure both, report in the commit message).

Left to the implementer: name scheme of the temp directory.

---

### Commit 2: Pass a stage list and stdio specs to Host::spawn

Purpose: change the Host spawn interface to S1 §5's shape with only the variants that exist today; no eml change.

Depends on: S1 §5 Host interface.

Files: `crates/eml_rt/src/host.rs`, `crates/eml_rt/src/lib.rs`, `crates/eml_rt/src/heap/tests/host.rs` (MemoryHost), `crates/eml_vm/src/hosts.rs`, `crates/eml_vm/tests/preempt.rs:48`.

Concrete change (replaces `Stdio`, host `StderrMode`, `Spawn`, `Spawned` [V] host.rs:1022-1093):
```rust
pub enum StdinSpec { Inherit, Null, Pipe }
pub enum StdoutSpec { Inherit, Null, Pipe }
pub enum StderrSpec { Inherit, ToStdout, Null }
pub struct Stage<'a> { pub program: &'a str, pub args: &'a [&'a str], pub env: &'a [(&'a str, &'a str)], pub cwd: Option<&'a str>, pub stderr: StderrSpec }
pub struct Spawn<'a> { pub stages: &'a [Stage<'a>], pub stdin: StdinSpec, pub stdout: StdoutSpec }
pub struct Spawned { pub stdin: Option<Box<dyn Write + Send>>, pub stdout: Option<Box<dyn Read + Send>>, pub processes: Vec<Box<dyn Process>> }
```
`Stage::dir`/`program_path` replace `Spawn`'s ([V] host.rs:1062-1078). `host_spawn` builds a one-stage `Spawn`; `Host::spawn` implementations refuse `stages.len() != 1` with `InvalidInput` (removed in commit 4). `host_spawn` takes `processes.pop()`.

Spec: object-model.md 「子のプロセス」 sentence on what `Host::spawn` receives (stages, stdio specs, `Spawned.processes`).

Tests: kind 3: `MemoryHost::spawn` ([V] heap/tests/host.rs:308), `crates/eml_vm/tests/preempt.rs:48` and `CaptureHost::spawn` move to the new types with the same assertions.

Verification: `cargo nextest run`; eml_rt gate (Verus, verusfmt; Miri `trusted::` both models because the trusted child tests cross the Host boundary).

Left to the implementer: none.

---

### Commit 3: Add stdin and stdout modes to Cmd

Purpose: here-string stdin (G-3), `< in`, `> out`, `>> out` for `Proc` (single stage), `SPAWN_LOCK`, `Fault::Launch`.

Depends on: S1 A2 (types), A3, A4 (redirection); D1, D2, D9, D10; commits 1, 2.

Files: `std/Prelude.em`, `std/Proc.em`, `crates/eml_extern/src/lib.rs` (`ProcSpawn` row), `crates/eml_rt/src/{host.rs,externs.rs,heap/ctx/proc.rs,lib.rs}`, `crates/eml_rt/src/heap/tests/{ctx.rs,script.rs,host.rs,trusted.rs}`, `crates/eml_vm/src/{hosts.rs,vm.rs}`, `crates/eml_core_ir/tests/externs.rs`.

Concrete change:
- Prelude: `StdinMode = DefaultStdin | NullStdin | StdinText String | StdinFile String`, `StdoutMode` (S1 A2), `Cmd` gains `stdin`, `stdout` between `env` and `stderr`; `cmd_new` sets `DefaultStdin`, `DefaultStdout`, `InheritStderr`; `show` prints `stdin = …, stdout = …` before `stderr`.
- host.rs: `StdinSpec` gains `Text(&'a [u8])`, `File(&'a str)`; `StdoutSpec` gains `File { path: &'a str, mode: WriteMode }`; `pub static SPAWN_LOCK: Mutex<()>`; `pub struct LaunchError { pub stage: usize, pub cause: LaunchCause }` (`impl Error`), `pub enum LaunchCause { StdinText, Open(Box<str>) }`; `pub fn here_file(bytes: &[u8]) -> io::Result<fs::File>`; `pub fn redirect_file(root: &Path, path: &str, mode: Option<WriteMode>) -> io::Result<fs::File>` (`FileIn.open`/`FileOut.create` rules incl. `directory_error`); `pub fn launch(host: &HostRef, control: &Control, stages: &str, stdin: i64, stdin_payload: &str, stdout: i64, stdout_path: &str) -> Result<Launched, Fault>`, `pub struct Launched { pub programs: Vec<Box<str>>, pub spawned: Spawned }`. `host_spawn` (stays `external_body`, same name, so src/trust.rs:168 keeps its row [V]) = `launch` + `Child`.
- `launch`: stop-bit check and `host.flush()` as `host_spawn` today ([V] host.rs:441-445); decode one stage (5 fields: program, args list, env list, cwd, stderr code; more stages -> `InvalidInput` until commit 4); codes stdin 0 inherit/1 null/2 pipe/3 text/4 file, stdout 0 inherit/1 null/2 pipe/3 file/4 append, stderr 0 inherit/1 merge/2 discard; error mapping per D1.
- Here-file: unix `OpenOptions::new().read(true).write(true).create_new(true).mode(0o600)` on `temp_dir()/eml-stdin-<pid>-<n>` (`n` from a static `AtomicU64`, retry on `AlreadyExists`), `fs::remove_file`, `write_all`, `seek(Start(0))`; Windows S1 A3 recipe with local constants `GENERIC_READ = 0x8000_0000`, `GENERIC_WRITE = 0x4000_0000`, `DELETE = 0x0001_0000`, `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE = 7`, `FILE_FLAG_DELETE_ON_CLOSE = 0x0400_0000` [I, Win32 constants]. Failure -> `LaunchError { stage: 0, cause: StdinText }`.
- `StdHost::spawn` and `CaptureHost::spawn`: flush output, then take `SPAWN_LOCK`, create here-file/redirect files, spawn, drop the `Command`, release; `CaptureHost` creates its drain threads after the release. `CaptureHost` resolves `StdinSpec::File`/`StdoutSpec::File` paths through the D7 store first (a written path reads its store file; `> out` writes a store file), else `redirect_file` under `file_root`. Failures wrap as `LaunchError { stage: 0 or last, cause: Open(path) }`.
- externs.rs: `Fault::Launch { program: Box<str>, cause: Box<LaunchCause>, reason: IoReason }` (`LaunchCause` defined in host.rs and used here, both outside proofs); display ``cannot run `{program}`: cannot make the standard input file: {reason}`` / ``cannot run `{program}`: cannot open `{path}`: {reason}``; `io_error_kind` = `reason_kind(reason)`; every `match` over `Fault` gets the arm next to `Io`.
- heap/ctx/proc.rs: `proc_spawn(stages: Word, stdin: i64, stdin_payload: Word, stdout: i64, stdout_path: Word)`. Proof: copy today's 4-word proof with one word fewer: new `open spec fn holds_texts3(a, b, c)` (`is_string` each + `holds_args(seq![a, b, c])`), `ws = seq![stages, stdin_payload, stdout_path]`, `lemma_holds_one(ws, 0..=2)`, three `checked_body`/`lemma_settled_body` calls, the `evolved_by` closure with three `-points` terms; `holds_texts` stays for its other callers.
- vm.rs `Extern::ProcSpawn` arm: 5 arguments.
- Proc.em:
```haskell
-- `Std.Exec` のためのインターフェースで、`Std.Net` のための `io_wait` と同じ理由で `pub` である
pub launch_args : Int -> Int -> Bool -> Cmd -> <IO> (String, Int, String, Int, String)
pub stdin_text : String -> Cmd -> Cmd
pub stdin_file : String -> Cmd -> Cmd
pub no_stdin : Cmd -> Cmd
pub stdout_file : String -> Cmd -> Cmd
pub stdout_append : String -> Cmd -> Cmd
extern spawn_raw : String -> Int -> String -> Int -> String -> <IO> Child
run c = wait (spawn_from 0 0 c)    -- output/capture: spawn_from 1 2; spawn: spawn_from 2 2
spawn_from i o c = let (s, si, sp, so, sop) = launch_args i o False c in spawn_raw s si sp so sop
```
`launch_args` maps `DefaultStdin`/`DefaultStdout` to the given codes; its `Bool` (whether the last stage may carry `PipeStderr`) is `False` at every call until commit 12 gives it meaning. A redirected stdout under `output`/`capture` reads `""` (document).

Spec: stdlib.md 「`Cmd`」 (new fields and modes, setters, launch defaults), 「`Proc`」 (stdin text as an unnamed seekable regular file, `no_stdin`, redirections relative to `file_root`, failure rows), 「実行時エラー」 rows for `Launch`; expressions.md 「コマンドリテラル」 defaults bullet; object-model.md 「子のプロセス」 (`SPAWN_LOCK` rule D10, here-file), `Fault` table row `Launch` (object-model.md:1014 area), T11 row (`launch`, `here_file`, `redirect_file`); lir.md:835 runtime error text list. The rule, stated once in stdlib.md 「`Proc`」 失敗: launch errors (``cannot run `p`: …``, including the here-file and redirect forms) give one of the four reason words (`not found`, `permission denied`, `not available`, `I/O error`), never the OS text; read, write and wait failures of `Std.Stream` and `Std.Exec` keep the OS text as `Std.Net`'s do. object-model.md 「子のプロセス」 also states that `SPAWN_LOCK` covers only eml's own spawns: a thread outside eml that forks or spawns can inherit a descriptor in the Apple `pipe` + `FD_CLOEXEC` window.

Tests:
- New UI: `tests/ui/run/std/proc_stdin_text.em` (child `eml run proc_stdin_text.em child` prints byte length and line count of `Stdin.read_all`; parent sends 2 MiB built in eml with `capture`, then 3 lines with `output`); `tests/ui/run/std/proc_redirect.em` with side input `tests/ui/run/std/proc_redirect.input` (`stdin_file`, `stdout_file "out.txt"`, `stdout_append "out.txt"`, then `FileIn.open "out.txt"`; snapshot shows `--- written: out.txt ---`); `tests/ui/run/io/try_io_redirect_missing.em` (`NotFound: cannot run `eml`: cannot open `nope.txt`: not found`, no child starts).
- New Rust: `crates/eml_rt/src/heap/tests/host.rs` `a_here_file_reads_back_its_bytes_and_has_no_name` (unix: no `eml-stdin-*` left in `temp_dir()`), `a_launch_error_names_the_file_it_could_not_open`.
- Kind 2: the two snapshots with `stderr = InheritStderr` (Q7 item 1); `crates/eml_core_ir/tests/externs.rs` row `"Std.Proc.spawn_raw" => (&[Unr; 5], Lin)`.
- Kind 3: callers of `proc_spawn` in `crates/eml_rt/src/heap/tests/{ctx.rs,script.rs,trusted.rs}`.

Verification:
- `EML_UI_FILTER=proc_stdin_text cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)'` -> 1 test passed; `crates/eml_cli/tests/snapshots/integration__ui__run@std__proc_stdin_text.em.snap` exists.
- `EML_UI_FILTER=proc_redirect cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)'` -> 1 test passed; `git status --porcelain -- tests/` prints nothing.
- `git grep -n "windows_sys\|libc::" -- crates/eml_rt/src/host.rs` and `git grep -n "thread::spawn\|spawn_masked" -- crates/eml_rt/src/host.rs` print nothing.
- `git grep -n "Async" -- std/Proc.em` prints nothing.
- `cargo nextest run -p eml_vm --test integration -E 'test(bench::)'` -> passes with no snapshot change (review C-8).
- eml_rt gate incl. Miri `trusted::` both models.

Left to the implementer: exact `Show` spacing (must equal the derived record form).

---

### Commit 4: Spawn a Cmd pipeline in one Host call

Purpose: `.|`, `StdinFrom`, multi-stage spawn, `Child` with stages, `Proc.wait` reaping every stage, `Proc.wait_all`, the middle-stage launch error.

Depends on: S1 A2 (setter rule, semantics); D1, D8, D10; commit 3.

Files: `std/Prelude.em` (`StdinFrom Cmd`; `display` joins stages with ` | `, head first), `std/Proc.em`, `crates/eml_extern/src/lib.rs` (`ProcStageCount`, `ProcWaitStage`), `crates/eml_rt/src/{host.rs,heap/ctx/proc.rs,trust.rs}`, `crates/eml_vm/src/{hosts.rs,vm.rs}`, `crates/eml_core_ir/tests/externs.rs`.

Concrete change:
- `launch` decodes n >= 1 stages. `StdHost::spawn`/`CaptureHost::spawn` under `SPAWN_LOCK`: for k in 0..n: stdin = (k == 0 ? spec.stdin : reader of pipe k-1), stdout = (k == n-1 ? spec.stdout : writer of a fresh `std::io::pipe()`), stderr per stage (`ToStdout` on a middle stage gets a `try_clone` of its stdout writer); spawn; drop the `Command`. Stage k failing: kill and wait stages < k, return the error wrapped as `LaunchError { stage: k, cause: LaunchCause::Spawn }` (new variant) unless it already carries a `LaunchError`. `CaptureHost` drains `Inherit` stderr of every stage and `Inherit` stdout of the last.
- host.rs `Child { programs: Vec<Box<str>>, processes: Vec<Option<Box<dyn Process>>>, codes: Vec<Option<i32>>, stdin, stdout }`; `Drop`: drop stdin and stdout, kill every unreaped stage, wait each head first. `child_wait`: drop stdin, reap unreaped stages head first, return the last code. New `external_body` `child_wait_stage(c, k, control)` (drop stdin, reap stages 0..=k in order, return stage k's code) and `child_stage_count(c)`.
- Deadlock rule (review C-5): `wait` and `wait_all` reap head first. With the last stage's stdout read to its end (or not a pipe), every stage can finish, so head-first reaping ends; with an unread pipe stdout the last stage can block, and no reaping order avoids that. This is the existing rule 「`wait` の前に標準出力を読み切る」 ([V] stdlib.md:744-748), stated for pipelines.
- `launch_args`: a non-last stage with `stdout /= DefaultStdout` -> `fail_io Other "cannot run `\{stage.program}`: the standard output of a stage that is not last is redirected"`.
- `(.|) left right` sets the head stage of `right` to `StdinFrom left`; `stdin_text`/`stdin_file`/`no_stdin` walk to the head (S1 A2).
```haskell
pub infixl 2 .|
pub wait_all : Child -> <IO> List Int
wait_all c =
  let (n, c) = stage_count c
  wait_from 0 n c Nil
wait_from k n c acc =
  if k + 1 == n then List.reverse (wait c :: acc)
  else
    let (code, c) = wait_stage k c
    wait_from (k + 1) n c (code :: acc)
extern stage_count : Child -> (Int, Child)
extern wait_stage : Int -> Child -> <IO> (Int, Child)
```

Spec: stdlib.md 「`Proc`」 (`.|`, `infixl 2`, setter rule both sentences, last-stage code, `wait_all`, no `pipefail`, 141 in `wait_all` only, middle-stage error, drop kills every stage, the pipe rules of 「`Child`」 apply to the last stage's stdout and to the head's stdin), 「`Cmd`」 (`show`/`display` of a pipeline, `env` per stage); object-model.md 「子のプロセス」 (stages, drop order; children start with SIGPIPE at `SIG_DFL` as std sets it, eml keeps Rust's `SIG_IGN`, and an embedding must keep SIGPIPE ignored, or a write to a closed pipe kills the process) and T11 row.

Tests:
- New UI: `tests/ui/run/std/proc_pipeline.em`:
  (a) 3-stage `eml` pipeline; the head writes 1 MiB of lines with `discard_stderr`, the middle copies stdin to stdout with `discard_stderr`, the last reads one line and exits; prints `capture`'s text only (head and middle codes and stderr are timing dependent and are neither printed nor captured);
  (b) a 2-stage pipeline whose head writes one line and exits 3: prints `wait_all` = `[3, 0]` and `run` of the same pipeline = `0`;
  (c) `stdin_text "x" (a .| b)` and `stdin_text "x" a .| b` give the same output.
  `tests/ui/run/std/proc_pipeline_show.em` (no children): `show` and `display` of a pipeline. `tests/ui/run-fail/std/proc_stage_redirect.em` (no children): `Proc.run (Proc.stdout_file "x" `eml-first` .| `eml-second`)` -> ``cannot run `eml-first`: the standard output of a stage that is not last is redirected`` (distinct program names, review C-4).
- New Rust: `crates/eml_rt/src/heap/tests/trusted.rs` `a_dropped_pipeline_kills_and_waits_every_stage` (MemoryHost); `crates/eml_rt/src/heap/tests/host.rs` `a_failed_stage_kills_the_stages_before_it` and `a_failed_stage_names_its_program`; `crates/eml_cli/tests/cli.rs` `a_pipeline_runs_through_the_real_host`.
- Kind 2: `crates/eml_core_ir/tests/externs.rs` new rows in both tables and the doc-comment counts (`Child` の11 -> 13).

Verification:
- `EML_UI_FILTER=proc_pipeline cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)'` -> 1 test passed; snapshots `integration__ui__run@std__proc_pipeline.em.snap` and `…proc_pipeline_show.em.snap` exist.
- `EML_UI_FILTER=proc_stage_redirect cargo nextest run -p eml_cli --test integration -E 'test(=ui::run_fail)'` -> 1 test passed; its snapshot contains ``cannot run `eml-first`: the standard output of a stage that is not last is redirected``.
- `git grep -n "SPAWN_LOCK" -- crates/eml_rt/src/host.rs crates/eml_vm/src/hosts.rs` shows one `.lock()` in each `spawn`.
- eml_rt gate incl. Miri `trusted::` both models; src/trust.rs gains `("host.rs: external_body child_wait_stage", "T11")`, `("host.rs: external_body child_stage_count", "T11")`.

---

### Commit 5: Run regular-file reads and writes as pool jobs

Purpose: the eml_rt file interface and the core's file jobs using the lent `RawBuf` inside the OS call (G-5), `Memory`-held, the T10 revision with its Miri tests (G-10), `Op::Adopt` (D3).

Depends on: S1 B8, B9 (regular files), B10; D3; commit 4 (only for order).

Files: `crates/eml_io/Cargo.toml` (windows-sys feature `Win32_Storage_FileSystem` for `ReadFile`/`WriteFile` of the Windows `file_job` [V] windows-sys-0.61.2 Storage/FileSystem/mod.rs:332,464; `OVERLAPPED` comes from the present `Win32_System_IO`), `crates/eml_rt/src/{driver.rs,lib.rs,heap/tests/trusted.rs}`, `crates/eml_io/src/{driver/mod.rs,driver/backend.rs,driver/pool.rs,driver/tests/mod.rs,driver/tests/fake.rs,real.rs,test.rs,sys/unix.rs,sys/iocp.rs}`, `crates/eml_io/tests/real.rs`, `crates/eml_vm/src/cells.rs` (`Bind::Unbound(RawResource)`, fake driver), `docs/spec/{object-model.md,vm.md}`, `docs/implementation/testing.md`, `.github/workflows/ci.yml`, `CLAUDE.md` (TSan command).

Concrete change (driver.rs):
```rust
#[cfg(unix)] pub type OsHandle = std::os::fd::OwnedFd;  #[cfg(windows)] pub type OsHandle = std::os::windows::io::OwnedHandle;
pub struct RawFile(OsHandle);                       // new/into_inner, Debug
pub enum RawResource { Socket(RawSocket), File(RawFile) }   // pipes added in commit 8
pub enum Op { /* existing */ ..., Adopt { res: RawResource } }
pub enum Done { /* existing */ ..., Handle(IoHandle) }
trait IoDriver { ...; fn adopt(&mut self, res: RawResource) -> io::Result<IoHandle>; }
```
- `OsHandle` lives in driver.rs; host.rs uses it from commit 11.
- Core: `Handle<S>` gains `class: Class { Socket, File }`, read by `submit_io` to choose steps or a file job; `Op::Adopt` -> `Submit::Done(self.adopt(res).map(Done::Handle))`; `Kind::FileRead { handle, buf }`, `Kind::FileWrite { handle, buf }` with `owner()` = `(handle, Read/Write)`, submitted with `claim`, a ticket and `pool.submit(backend.file_job(&sock, write, buf))`, slot `Hold::Memory`, `Submit::Pending`; `cancel` of a file job is a no-op; `answer_arrived` gives `Done::Size(n)`; `Drop` keeps the tickets of `Memory`-held job slots and waits for their answers through `turn` up to `HELD_LIMIT` (1 s), then `abort` with `HELD_ABORT`, never calling `backend.cancel` on a job.
- `Backend::adopt(RawResource)`; `Backend::file_job(&self, sock: &Self::Sock, write: bool, buf: RawBuf) -> pool::Job`: unix captures the `RawFd` and `RawBuf`, read = one `libc::read` retried on `EINTR`, write = `libc::write` loop until `cap` bytes (`WriteZero` on 0); Windows captures the handle as `isize`, `ReadFile`/`WriteFile` with a null `OVERLAPPED`, write loops. Pointer only inside the call, no slice (unix.rs and iocp.rs are in `UNSAFE_FILES` [V] trust.rs:33-41). `adopt(File)`: unix keeps the fd with no row registration; IOCP keeps the handle with no port association. Fake backend `file_job` writes with `copy_nonoverlapping`.
- `TestDriver`: `adopt` of a `File` and `Op::Adopt` -> `Unsupported` until commit 6 (one sentence in vm.md, rewritten in commit 6).
- cells.rs: `Bind::Unbound(RawResource)`; `bind_stream` adopts the `RawResource` it holds (only sockets exist in this commit).
- driver.rs `RawBuf` doc and `SAFETY` comment ([V] driver.rs:18-33): the B10 rule.
- When drop can abort (review C-6, verified): every run ends with `quiesce` before the driver drops: worker k >= 1 in `retire_driver` before returning `Ctx` ([V] workers.rs:1250-1262), worker 0 after joining on success, error and `exit` ([V] lib.rs:1044-1060, vm.md:346 「誤りで終わった run と `exit` が捨てた run でも」), and `recover` before touching the heap ([V] lib.rs:511-518). `quiesce` waits for jobs without limit unless the driver is broken ([V] driver/mod.rs:823-873 returns early only when `broken`). So the 1 s abort is reachable only when (a) the driver broke (OS wait failure, refused cancel, cancel timeout) while a file job ran, which also poisons the `Machine` and leaks the heap ([V] lib.rs:832-843), or (b) an embedder drops a `RealDriver` without `quiesce` (the trait's documented contract [V] driver.rs:150-155). A slow disk in a normal run never reaches it.

Spec (G-10, all in this commit): object-model.md T10 row (pool threads of the driver running the op's file job are external writers; the pool queue mutex and the inbox mutex give the happens-before edges; Miri column adds the threaded write); vm.md 「時計と driver」: `RawBuf` bullet (S1 B10 text), `Job` bullet (file jobs, `Memory` hold, drop waits 1 s then aborts, cancel no-op, when that abort is reachable), `RawResource`/`RawFile` bullet, `Adopt`/`Done::Handle` (answered at submit by the driver that receives it, through its `adopt`; the test driver refuses it), `adopt` of `File`; testing.md 「ThreadSanitizer」 (adds `-p eml_io --lib -- driver::tests::`), 「核の Miri」 (file-job tests, pool limit 2, re-measured budget), 「よく使うコマンド」 TSan line; ci.yml `tsan` job step; CLAUDE.md TSan command comment.

Tests (new):
- `crates/eml_rt/src/heap/tests/trusted.rs` `a_lent_room_is_written_by_another_thread` (`both_modes!`, `LARGE_BODY + 3`, `std::thread::scope`, raw-pointer write, join, `bytes_commit`, read back).
- `crates/eml_io/src/driver/tests/mod.rs`: `the_adopt_op_answers_at_submit_with_a_handle`, `a_file_read_job_fills_the_lent_room`, `a_file_write_job_reads_the_lent_bytes`, `a_dropped_driver_waits_for_a_file_job`, `a_cancel_does_not_end_a_file_job`, `a_closed_file_waits_for_its_running_job`.
- `crates/eml_io/tests/real.rs`: `a_file_is_written_and_read_through_the_pool`, `dropping_a_driver_waits_for_a_running_file_job`.
- Kind 3: `adopt(sock)` -> `adopt(RawResource::Socket(sock))` in `crates/eml_io/tests/real.rs:1719,1776`, `crates/eml_io/src/driver/tests/*`, cells.rs tests and fake driver.

Verification:
- `cargo nextest run -p eml_io --lib -E 'test(driver::tests::a_dropped_driver_waits_for_a_file_job)'` -> 1 passed.
- `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` and with `-Zmiri-tree-borrows` added: pass within 280 s / 365 s `user`.
- `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib -- driver::tests::` and with `-Zmiri-tree-borrows`: pass; the new `user` times go into testing.md 「核の Miri」.
- TSan (CI): `… cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_io --lib -- driver::tests::` -> no report. ASan job (CI) for `eml_io`.
- `git log --format=%H -S"FileRead" -- crates/eml_io/src/driver/mod.rs | tail -1 | xargs git show --stat` lists `docs/spec/object-model.md`, `docs/spec/vm.md`, `crates/eml_rt/src/heap/tests/trusted.rs`, `crates/eml_io/src/driver/tests/mod.rs`.
- `grep -rn "from_raw_parts" crates/eml_io/src | grep -v "src/test.rs\|src/trust.rs"` prints nothing.
- eml_rt gate.

Left to the implementer: the exact `turn` loop in `Drop`.

---

### Commit 6: Model regular files in the test driver

Purpose: deterministic file ops on the virtual clock (G-8).

Depends on: S1 A7, B12, C-3; commit 5.

Files: `crates/eml_io/src/test.rs`, `crates/eml_io/tests/test_driver.rs`, `docs/spec/vm.md`.

Concrete change: `files: HashMap<IoHandle, fs::File>`; `Op::Adopt { res: File(f) }` and `adopt(File)` -> handle from the network's counter (one handle space [V] R1 §4), `Submit::Done(Ok(Done::Handle(h)))`; `adopt(Socket)` stays `Unsupported`; `Read`/`Write` on a file handle are queued like `Op::Job`, executed in `try_finish` at settle with `Read::read` into the lent pointer / `Write::write_all` from it, never finished in `submit`, never cancellable (`cancel` leaves them; result delivered with `cancelled: false`), run by `quiesce` with results dropped as jobs are; `close`/bin of a file handle drops the `File`.

Spec: vm.md 「時計と driver」 test-driver bullet (files; jobs-like settling).

Tests (new, `crates/eml_io/tests/test_driver.rs`): `a_file_read_settles_at_wait`, `a_cancelled_file_read_still_delivers_its_bytes`, `quiesce_runs_a_pending_file_write`, `a_binned_file_handle_is_closed`, `a_seed_does_not_change_what_a_file_read_returns`.

Verification: `cargo nextest run -p eml_io --test integration -E 'test(test_driver::a_cancelled_file_read_still_delivers_its_bytes) | test(test_driver::a_binned_file_handle_is_closed)'` -> 2 passed. (`test_driver::the_test_driver_has_no_pipes` lands in commit 8, where `pipe` and `Op::Wait` exist.)

---

### Commit 7: Add Std.Stream

Purpose: user-facing async regular files.

Depends on: S1 A6 (amended), A7, B8 (grow-at-submit), B11 (amended); D3, D4, D5, D6, D11; commits 5, 6.

Files: `std/Stream.em` (new), `crates/eml_hir/src/lib.rs` (`STD` row `("Stream.em", include_str!(…))` [V] lib.rs:26,54), `crates/eml_extern/src/lib.rs` (types `StreamReader`, `StreamWriter`; 14 rows; `is_async`), `crates/eml_rt/src/{host.rs,lib.rs,heap/tests/host.rs}` (`open_raw`), `crates/eml_vm/src/{cells.rs,vm.rs,hosts.rs}`, `crates/eml_vm/tests/api.rs`, `crates/eml_core_ir/tests/externs.rs`, tests and docs below.

Concrete change:
- host.rs: `pub enum OpenMode { Read, Create, Append }`; `Host::open_raw(&self, path, mode) -> io::Result<RawFile>` default `Err(Unsupported)`; `pub const NOT_REGULAR: &str = "not a regular file"`; `StdHost::open_raw` opens `file_root.join(path)`; unix `custom_flags(O_NONBLOCK | O_NOCTTY)` with local constants `#[cfg(target_os = "linux")]` `O_NONBLOCK = 0o4000`, `O_NOCTTY = 0o400` [I, asm-generic fcntl.h], `#[cfg(target_vendor = "apple")]` `0x0004`, `0x0002_0000` [V] MacOSX.sdk sys/fcntl.h:113,137, and `#[cfg(all(unix, not(any(target_os = "linux", target_vendor = "apple"))))] compile_error!(…)` as eml_io's sys/mod.rs does [V R1 §3.1]; Windows plain `OpenOptions` (no flags); then D11. `CaptureHost::open_raw`: `Read` of a stored path opens its store file, else `StdHost::open_raw`; `Create`/`Append` open store files (D7).
- cells.rs: `NetStream` -> `IoStream { bind, origin, rest, start }`, `Origin { Peer(SocketAddr), File(Arc<str>) }`; `peer_addr` reads `Peer`. `OpKind::Read`/`Line`/`Write` gain `prefix: Prefix` with `Prefix { Net, File(Arc<str>, FileDir) }` (`Net` keeps today's texts); `kind.prefix()` renders ``cannot read `p` `` / ``cannot write to `p` `` for files. `OpKind::Open { path: Arc<str>, write: bool }`, prefix ``cannot open `p` ``.
- `submit_open(path, mode)`: `host.open_raw` on the calling worker; failure -> pre-submit `Raise::Io` (kind `io_kind(e)`, so `NOT_REGULAR` -> `NotAvailable`; message ``cannot open `<path>`: <reason word>`` or ``…: not a regular file``) with no record, as `submit_connect`'s pre-submit errors ([V] vm.rs:3083-3092); success -> `sched.own_driver()?` (real clock), record `Open`, op `Op::Adopt { res: RawResource::File(f) }`, host = the caller's I/O host.
- `Open` settle: from `Done::Handle(h)` build `made = IoStream { bind: Bound { host: record.host, handle: h, bin }, origin: File(path) }` whether or not the record is `cancelling`, as the `Connect` arm does ([V] cells.rs:3433-3452); `finish` drops `made` (its `Drop` bins the handle to its host [V] cells.rs:795-800); `sweep_all` discards an untaken `made` ([V] cells.rs:2741-2752 per ruling.md).
- Read submit rule: socket streams unchanged; file streams lend `spare_room` if present, else `grow_read_room` (read) / `lend_line_room(.., grow = true)` (line, also in `LineStep::Again`) at submit, phase `Transfer`; `Op::Readable` is built only in the socket arms.
- vm.rs `Async` arm: `StreamSubmitOpen`, `StreamSubmitRead`/`ReadLine`/`Write` (shared bodies with Net's), `StreamOpReader`/`StreamOpWriter` (shared with `NetOpStream`), `StreamOpOutcome`, `StreamOpPrefix`, `StreamTakeReader`/`TakeWriter`, `StreamTakeBytes`/`TakeLine`, `StreamFinish` (shared), `StreamSource`; the fallback text and the generic arm's list ([V] vm.rs:3196-3199, 4311-4316) gain them.
- std/Stream.em:
```haskell
read_chunk b r =
  if Async.cancel_requested () then
    drop r
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_read b r)
    let (r, op) = op_reader op
    let (code, op) = op_outcome op
    match w with
      | Async.Cancelled ->
        finish op
        drop r
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then (take_bytes op, r)
        else
          let (prefix, op) = op_prefix op
          finish op
          drop r
          fail_code code prefix
open path = opened 0 path     -- create: 1, append: 2; Cancelled -> finish op (drops `made`), raise_cancelled; Woke 0 -> take_reader/take_writer
fail_code : Int -> String -> <IO> a     -- 2 -> ConnectionReset "\{prefix}: broken pipe"; 3, 4, 5 as Net.em:248-251
raise_cancelled : Unit -> <IO> a        -- Net.em:240 と同じ
read_all_bytes r = read_chunk on one growing Bytes until a chunk adds 0 bytes
read_all r = read_all_bytes, then decode; failure -> fail_io NotUtf8 "cannot read \{name}: the text is not valid UTF-8", `name` from `source`
write s w = let (_, w) = write_bytes (Bytes.of_string s) w in w; write_line s w = write (s ++ "\n") w
close r = drop r; close_writer w = drop w
```

Spec: stdlib.md 「`Std.Stream`」 (new, after 「`Std.Net`」: types, functions, regular files only, directories refused on every OS, `file_root`, failures and prefixes, a running job cannot be cancelled, `Async.timeout` around a file op, no buffer, no `flush`), 「実行時エラー」 rows; vm.md 「操作の記録」 (open records, settle like `connect`, cancelled opener's handle binned by `finish`, grow-at-submit for file reads, `IoStream` origins), 「時計と driver」 (files bound by `Adopt` on the opener's I/O host; the file's host is fixed at open; rescue moves the owner, never the host; non-host owners go by `Submit`/`Completion` mail; `Op::Adopt` is answered at submit by the host's driver, so the opener parks only when its I/O host is another worker (mail round trip), and then settles like `connect`; a task parked on an `Open` record is never rescued (「救済」)); object-model.md T11 row (`open_raw`); modules.md:69 std list; async.md:261, vm.md:523, roadmap.md 「プールの仕事の予算と、放さずに終わる方針」 and `ffi-blocking` (:39): replace "今は std に仕事を出す行がなく" with the file jobs of `Std.Stream`; status.md:147 (driver users).

Tests:
- New UI (virtual clock, seeds, N = 2/8; each starts a task and spins for the preemption check [V] testing.md 「seed で流す」): `tests/ui/run/async/stream_read_file.em` + `stream_read_file.txt`, `stream_write_read_back.em` (snapshot has `--- written: … ---`), `stream_cancelled_reader.em`, `stream_timeout_after_job.em`, `stream_open_special.em` (`Stream.open "."` and `Stream.create "."` -> `NotAvailable: cannot open `.`: not a regular file`, no Windows mark), `stream_open_cancelled.em` (a task cancelled while parked at `open`'s `io_wait`; the run ends normally; on the virtual clock at N = 2/8 the open is hosted by worker 0); `tests/ui/run-fail/async/stream_missing_file.em`.
- cells.rs unit tests: `a_read_of_a_file_grows_at_submit`, `a_read_line_of_a_file_grows_at_submit`, `a_read_of_a_shared_buffer_on_a_file_copies_once` (Miri set: names start `a_read` [V] testing.md:199), `an_open_record_binds_the_file_to_the_openers_host` (N = 2 fake driver: worker 1's record gives `Bound { host: 0 }` on the virtual clock, `host: 1` on the real clock), `an_open_cancelled_after_adoption_bins_its_handle`.
- `crates/eml_rt/src/heap/tests/host.rs`: `a_raw_open_refuses_a_directory_as_not_a_regular_file` (read and create), `a_raw_open_of_a_missing_file_is_not_found`.
- `crates/eml_vm/tests/api.rs`: `a_real_file_is_written_and_read_back` (Clock::Real, N = 1 and 4), `a_stream_open_of_a_fifo_fails_at_once` (`#[cfg(unix)]`, `libc::mkfifo` in a temp dir, `StdHost`).
- Kind 2: `crates/eml_core_ir/tests/externs.rs` tables; kind 3: `NetStream` -> `IoStream` in cells.rs tests.

Verification:
- `EML_UI_FILTER=async/stream_ cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'` -> 2 tests passed; 7 `stream_*` snapshots exist.
- `cargo nextest run -p eml_cli --test integration -E 'test(ui::)' && git status --porcelain -- tests/` -> tests pass, status prints nothing.
- `grep -n "written: " crates/eml_cli/tests/snapshots/integration__ui__run@async__stream_write_read_back.em.snap` -> at least one line.
- `git grep -n "O_NONBLOCK" -- crates/eml_rt/src/host.rs` shows the open flags.
- `cargo nextest run -p eml_vm --lib -E 'test(cells::an_open_record_binds_the_file_to_the_openers_host) | test(cells::an_open_cancelled_after_adoption_bins_its_handle)'` -> 2 passed.
- `git grep -n "op_failed" -- std/` prints nothing; `git grep -n "op_prefix" -- std/Stream.em` shows one non-`pub` `extern` row and its uses in the `Woke` branches; `git grep -n "Async.OpId" -- std/Stream.em` shows only non-`pub` `extern` rows.
- `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write counters::` and with `-Zmiri-tree-borrows` -> pass.
- eml_rt gate (Verus, verusfmt). ASan/TSan jobs (CI) for `eml_vm --test integration -- api::`.

Left to the implementer: the `Bytes`-to-`String` function behind `read_all`.

---

### Commit 8: Add pipes and child-exit waits to the epoll and kqueue backends

Purpose: G-4, G-7 on unix; core `Wait` step; the pipe and process types.

Depends on: S1 B8, B9 unix bullets, A-1; D10; commit 5.

Files: `crates/eml_rt/src/{driver.rs,lib.rs}`, `crates/eml_io/src/{driver/mod.rs,driver/backend.rs,driver/tests/mod.rs,driver/tests/fake.rs,real.rs,test.rs,sys/unix.rs,sys/epoll.rs,sys/kqueue.rs,sys/iocp.rs}`, `crates/eml_io/tests/{real.rs,test_driver.rs}`, `crates/eml_vm/src/cells.rs` (fake driver's `pipe`), `docs/spec/vm.md`.

Concrete change:
- driver.rs: `pub struct RawPipe(OsHandle)`; `#[cfg(unix)] pub struct RawProcess(pub i32)`, `#[cfg(windows)] pub struct RawProcess(pub OwnedHandle)`; `RawResource` gains `PipeRead(RawPipe)`, `PipeWrite(RawPipe)`; `pub struct PipePair { pub parent: RawPipe, pub child: OsHandle }`; `Op::Wait { process: RawProcess }` (`Done::Size(0)` once exited; never reaps); `IoDriver::pipe(&mut self, parent_reads: bool) -> io::Result<PipePair>`.
- Core: `Class::Pipe`; `Step::Wait { process: &RawProcess }`, `Kind::Wait { process }` (no owner), `Outcome::Size(0)`; `cancel` -> `backend.cancel(key, Step::Wait)`; `Backend::pipe`.
- unix.rs `pipe`: holds `eml_rt::SPAWN_LOCK` around creation and flag setup; Linux `pipe2(O_CLOEXEC)`, Apple `pipe` + `FD_CLOEXEC` on both; `O_NONBLOCK` on the parent end only. `adopt(PipeRead|PipeWrite)`: `register_row(fd, write = matches!(PipeWrite))`; `Sock` gets a class so `Step::Recv/Send` call `read(2)`/`write(2)`; `Step::Peek` on a pipe returns `InvalidInput` with a `debug_assert!` (the VM never sends `Readable` there). EPIPE -> `BrokenPipe`.
- Linux wait: `syscall(SYS_pidfd_open, pid, 0)`; `ENOSYS` -> `Start::Done(Err(Unsupported))`; register `EPOLLIN | EPOLLET` on a new row holding the pidfd; event -> `Arrival::Done(Ok(Size(0)))`; cancel closes the pidfd, `Cancel::Done`.
- macOS wait: fd-less row; `kevent(EVFILT_PROC, pid, NOTE_EXIT, EV_ADD | EV_ONESHOT, udata = token)`; `ESRCH` -> `Done`; then `waitid(P_PID, pid, WEXITED | WNOHANG | WNOWAIT)` reporting exit -> `EV_DELETE`, `Done`; kqueue.rs `poll` gains an `EVFILT_PROC` arm delivering `Ready { read: true, .. }`; cancel `EV_DELETE`, `Cancel::Done`.
- iocp.rs: `pipe`, `adopt(Pipe*)`, `Step::Wait` -> `Unsupported` (commit 9 implements them). `TestDriver`: `pipe`, `adopt(PipeRead|PipeWrite)`, `Op::Wait` -> `Unsupported` "the test driver has no processes".

Spec: vm.md 「時計と driver」: `RawPipe`/`RawProcess`/`PipePair`, `Wait` (never reaps), `pipe` (under `SPAWN_LOCK`, parent end nonblocking, child end blocking and close-on-exec), epoll/kqueue pipe registration, read/write instead of recv/send, pidfd, `EVFILT_PROC` + `WNOWAIT`; one sentence that the IOCP backend refuses `pipe` and `Wait` with `Unsupported` (rewritten in commit 9); the test driver refuses pipes and waits; pipe writes rely on SIGPIPE being ignored (EPIPE -> `BrokenPipe`), so an embedding of the real driver must keep SIGPIPE ignored.

Tests (new):
- `crates/eml_io/src/sys/unix.rs` `mod tests`: `a_pipe_parent_end_is_nonblocking_and_the_child_end_is_not` (`F_GETFL`), `a_pipe_end_is_close_on_exec` (`F_GETFD` both), `a_wait_on_an_unreaped_zombie_ends_at_once` (A-1).
- `crates/eml_io/tests/real.rs` (children = the test binary re-executed with `EML_IO_CHILD=<mode>` and `--exact real::child_main`, the pattern of driver/tests/abort.rs; `#[test] fn child_main()` returns at once without the variable): `a_pipe_reports_eof_after_the_child_exits`, `a_pipe_write_after_the_reader_exited_is_broken`, `a_pipe_read_is_cancelled_at_once`, `a_wait_on_an_exited_child_ends`, `a_wait_on_a_running_child_is_cancelled_at_once`, `a_wait_on_a_killed_child_ends`, `a_pipe_end_is_used_by_the_driver_of_another_thread`; all `#[cfg(unix)]` in this commit (removed in commit 9).
- `crates/eml_io/tests/test_driver.rs`: `the_test_driver_has_no_pipes`.

Verification:
- `cargo nextest run -p eml_io --lib -E 'test(sys::unix::tests::a_pipe_parent_end_is_nonblocking_and_the_child_end_is_not)'` -> 1 passed.
- `cargo nextest run -p eml_io --test integration -E 'test(real::a_wait_on_a_running_child_is_cancelled_at_once) | test(test_driver::the_test_driver_has_no_pipes)'` -> 2 passed.
- `git grep -nw "waitpid\|waitid\|WaitForSingleObject" -- crates/eml_io/src` -> only the `waitid` with `WNOWAIT` in sys/unix.rs and its test.
- `git grep -n "SPAWN_LOCK" -- crates/eml_io/src/sys` -> the lock in unix.rs `pipe`.
- eml_rt gate (Verus, verusfmt). ASan job (CI) for `eml_io`.

Left to the implementer: fd-less row layout; `libc` symbol names (`SYS_pidfd_open`, `waitid`, `P_PID`, `WNOWAIT`) [I].

---

### Commit 9: Add pipes and child-exit waits to the IOCP backend

Depends on: S1 B9 Windows bullets, C-1, C-6; commit 8.

Files: `crates/eml_io/Cargo.toml` (windows-sys features + `Win32_System_Pipes`, `Win32_Security` [V] windows-sys-0.61.2 Cargo.toml:137,230; `Win32_Storage_FileSystem` arrived in commit 5), `crates/eml_io/src/sys/iocp.rs`, `crates/eml_io/tests/real.rs` (remove commit 8's `#[cfg(unix)]`), `docs/spec/vm.md`.

Concrete change: `pipe` under `SPAWN_LOCK`: parent end `CreateNamedPipeW("\\.\pipe\eml-<pid>-<n>", (parent_reads ? PIPE_ACCESS_INBOUND : PIPE_ACCESS_OUTBOUND) | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT, 1, 65536, 65536, 0, null)`; child end `CreateFileW(name, parent_reads ? GENERIC_WRITE : GENERIC_READ, 0, null, OPEN_EXISTING, 0, null)` (synchronous, `SECURITY_ATTRIBUTES` null: not inheritable). `adopt(Pipe*)` associates with the port under `PIPE_KEY`; `Step::Recv/Send` on a pipe issue `ReadFile`/`WriteFile` with an `Over` box `What::PipeRead/PipeWrite`, `Hold::Memory`, cancel `CancelIoEx`; `ERROR_BROKEN_PIPE` on read -> `Ok(0)`; `ERROR_BROKEN_PIPE` and `ERROR_NO_DATA` on write -> `BrokenPipe`. Wait: `Exited { keys: Mutex<Vec<OpKey>>, waker: Arc<PortWaker> }`; `RegisterWaitForSingleObject(.., WT_EXECUTEONLYONCE)` with a boxed `(Arc<Exited>, OpKey)` context kept in the backend's per-key wait table together with the wait handle; the callback pushes the key and `wake`s; `wait` drains `keys` into `Event { key, Arrival::Done(Ok(Size(0))) }`; cancel removes the key from `keys` if present and gives `Cancel::Done`. Lifetime rule (one rule for fired and cancelled waits): the context box and the wait table entry are freed only after `UnregisterWaitEx(wait, INVALID_HANDLE_VALUE)` has returned, which blocks until a running callback has finished, so no callback can still use the box; a fired wait is unregistered when its key is drained, a cancelled one in `cancel`; a drained key alone never frees the box. Backend drop unregisters every remaining wait the same way.

Spec: vm.md 「時計と driver」 IOCP bullet (named pipes, `PIPE_KEY`, `Exited` list); remove commit 8's refusal sentence.

Tests (new, iocp.rs `mod tests`, Windows CI): `a_pipe_child_end_is_not_inheritable` (`GetHandleInformation`), `a_pipe_parent_end_is_overlapped`, `an_unregistered_wait_after_its_callback_ran_is_done`, `a_fired_wait_is_unregistered_before_its_context_is_freed`.

Verification: Windows CI `cargo nextest run -p eml_io --lib -E 'test(sys::iocp::tests::a_pipe_child_end_is_not_inheritable)'` -> passed; the real.rs pipe and wait tests pass on all three OSes; cross clippy for `x86_64-pc-windows-msvc` clean locally.

---

### Commit 10: Add the real-clock mark to UI tests

Depends on: S1 A7, B-5.

Files: `crates/eml_cli/tests/common/mod.rs`, `crates/eml_cli/tests/ui.rs`, `docs/implementation/testing.md`.

Concrete change:
- `Marks.real_clock: bool`; `leading_marks` reads `<leader> clock:` lines with `reason_mark(line, leader, kind, word)`, the generalisation of `skip_mark` ([V] common/mod.rs:228-245; `word` = `skip` for windows/workers, `real` for clock); malformed or reasonless -> `Err("the clock mark is `-- clock: real (<reason>)`, not `…`")`. `marks_of` rejects it outside run entries (`clock marks are not supported here`).
- ui.rs: `struct ExtraRuns { seeds: bool, workers: bool }`, `fn extra_runs(suffix: &str, marks: &Marks) -> ExtraRuns`: when `marks.real_clock`, asserts `suffix.starts_with("async/")` (`the clock mark is only for async/`) and `marks.signals.is_empty()` (`signal marks need the virtual clock`) and returns both false; otherwise `seeds = seeded(suffix)`, `workers = workers_run(suffix, marks.workers_skip)`. `run` and `run_fail_file` use it. `execute_in` adds `.with_clock(Clock::Real)` when `marks.real_clock` (`Clock` from `eml_cli` [V] eml_cli/src/lib.rs:20-21). Marked files run in checked and fast mode only, so no `RunStats`, `placed_tasks`, `rescued` or `preemptions` assertion runs for them.

Spec: testing.md 「UI テスト」 (the clock mark, only in `run/async/` and `run-fail/async/` entries; rules for marked files: at most 3 child launches, every child is `eml`, no `sleep_ms`/`timeout`/`_within`/`drain_within`, no dependence on which child finishes first), 「seed で流す」 (marked files have no seed and worker runs).

Tests (new, ui.rs): `the_clock_mark_needs_a_reason` (`leading_marks("-- clock: real ()\n", "--")` is `Err`), `a_clock_mark_outside_async_fails` (`#[should_panic(expected = "the clock mark is only for async/")]` on `extra_runs("io/x.em", &marked)`), `a_real_clock_file_has_no_seed_or_workers_runs` (`extra_runs("async/x.em", &Marks { real_clock: true, ..Default::default() })` is `ExtraRuns { seeds: false, workers: false }`), `a_clock_mark_in_a_transcript_fails` (`marks_of(.., "#", false)` panics).

Verification: `cargo nextest run -p eml_cli --test integration -E 'test(ui::the_clock_mark_needs_a_reason) | test(ui::a_clock_mark_outside_async_fails) | test(ui::a_real_clock_file_has_no_seed_or_workers_runs) | test(ui::a_clock_mark_in_a_transcript_fails)'` -> 4 passed; `git grep -n '^-- clock: real (' -- tests/ui` prints nothing.

---

### Commit 11: Add Std.Exec

Purpose: async child processes without the stderr pipe: `Exec.Child`, `spawn`/`run`/`output`/`capture`, `stdin`/`stdout` takers, `Op::Wait` records, `wait`, `wait_all`.

Depends on: S1 A4 (minus `PipeStderr`), A5, B11, G-12; D1, D2, D4, D5, D6; commits 7, 9, 10.

Files: `std/Exec.em` (new), `crates/eml_hir/src/lib.rs`, `crates/eml_extern/src/lib.rs` (type `ExecChild`; 11 rows minus `stderr`, i.e. 10; `is_async`), `crates/eml_rt/src/{host.rs,lib.rs}`, `crates/eml_vm/src/{cells.rs,vm.rs,hosts.rs}`, `crates/eml_vm/tests/api.rs`, `crates/eml_cli/tests/cli.rs`, `crates/eml_core_ir/tests/externs.rs`.

Concrete change:
- host.rs: `StdinSpec::Given(OsHandle)`, `StdoutSpec::Given(OsHandle)`; `launch` gains `pipes: Option<&mut dyn FnMut(PipeEnd) -> io::Result<OsHandle>>` and `pub enum PipeEnd { Stdin, Stdout }`; pipe codes become `Given(pipes(end)?)` when `Some`; `MergeStderr` with a given stdout gets a `try_clone` of that end. `Process::raw(&self) -> Option<RawProcess> { None }`; `OsProcess::raw` = `Child::id() as i32` / `as_handle().try_clone_to_owned()`. hosts.rs: `CaptureHost::spawn` handles `Given` with `Stdio::from(h)` and never drains it; `CapturedChild::raw` delegates to its `process::Child`.
- vm.rs `ExecSpawnRaw`: if `sched.driver_kind == DriverKind::Virtual`, raise `NotAvailable` ``cannot run `<last program>`: the test driver has no processes`` before touching any driver (on every worker; ruling.md Q1); else `sched.own_driver()?`, `launch(.., Some(&mut |end| driver.pipe(end == PipeEnd::Stdout).map(|p| { parents.push((end, p.parent)); p.child })))`; on `Err(fault)` drop `parents` and raise the fault as the generic arm does; on success alloc `ExecChild` with the parents as `Bind::Unbound(RawResource::PipeWrite/PipeRead)`.
- cells.rs: `Origin::Child { program: Arc<str>, pipe: ChildPipe }`, `ChildPipe { Stdin, Stdout }`; `Prefix::Child(Arc<str>, ChildPipe)` (``cannot write to `prog` ``, ``cannot read the output of `prog` ``), `Prefix::Wait(Arc<str>)` (``cannot wait for `prog` ``). `ExecChild { stages: Vec<ExecStage>, stdin: Option<Bind>, stdout: Option<Bind> }`, `ExecStage { program: Arc<str>, process: Option<Box<dyn Process>>, code: Option<i64> }`; `Drop`: close the untaken ends, kill every stage with a process, wait each, errors dropped. Pipe streams follow commit 7's file read rule (grow at submit, never `Readable`). `bind_here` before every submit on an unbound pipe ([V] vm.rs:3218-3227).
- Takers `stdin`/`stdout`: move the `Bind` into a new `IoStream { origin: Child { program, pipe } }` (program = head for stdin, last for stdout); absent -> raise `NotAvailable` ``cannot take the standard input of `prog`: not available`` (`… output …`).
- `submit_wait(k, child)`: drop an untaken stdin end; `OpKind::Wait { stage: k, prefix }`; record `owner = host = here.me`, refusing `here.host != here.me` with `CellFault::Internal("a wait is submitted on a worker that hosts no I/O")`; op `Op::Wait { process: stage.process.raw() }`; a reaped stage or `raw() == None` gives a `Done(Ok(0))` record without an op. `reap(k, child)`: `process.take().wait()` (immediate after the notice; blocking when `raw()` was `None`), stores and returns the code; failure raises ``cannot wait for `prog`: <reason word>``.
- vm.rs `Async` arm: `ExecSpawnRaw`, `ExecStdin`/`ExecStdout`, `ExecStageCount`, `ExecSubmitWait`, `ExecOpChild` (shared with `NetOpStream`), `ExecOpOutcome`, `ExecOpPrefix` (shared with `StreamOpPrefix`), `ExecFinish` (shared), `ExecReap`; fallback text and generic list updated.
- std/Exec.em:
```haskell
pub extern data Child
pub spawn c = launch 2 2 c; pub run c = wait (launch 0 0 c)
pub output c =
  let ch = launch 1 2 c
  if c.stdout == DefaultStdout then
    let (r, ch) = stdout ch
    let (s, r) = Stream.read_all r
    Stream.close r
    (s, wait ch)
  else ("", wait ch)          -- 向けた標準出力は `Proc.output` と同じく読まない
-- capture: nonzero last code -> fail_io Other "command `\{c.program}` exited with code \{show code}" (as Proc); output_bytes/capture_bytes likewise, with `Bytes.empty ()` for a redirected stdout
launch i o c =
  if Async.cancel_requested () then raise_cancelled ()
  else
    let (s, si, sp, so, sop) = Proc.launch_args i o False c
    spawn_raw s si sp so sop
wait_stage k c =
  if Async.cancel_requested () then
    drop c
    raise_cancelled ()
  else
    let (w, op) = Async.io_wait (submit_wait k c)
    let (c, op) = op_child op
    let (code, op) = op_outcome op
    match w with
      | Async.Cancelled ->
        finish op
        drop c
        raise_cancelled ()
      | Async.Woke ->
        if code == 0 then
          finish op
          reap k c
        else
          let (prefix, op) = op_prefix op
          finish op
          drop c
          fail_code code prefix
pub wait_all c = wait_stage over 0..n-1 (n from stage_count), then drop c; codes head first
pub wait c = the last code of wait_all c
pub extern stdin  : Child -> <IO> (Stream.Writer, Child)
pub extern stdout : Child -> <IO> (Stream.Reader, Child)
fail_code, raise_cancelled: as Stream.em
```

Spec: stdlib.md 「`Std.Exec`」 (new; S1 A4/A5 without the stderr pipe: drop, taken readers, EOF and grandchildren, cancellation, Ctrl-C, hosts without `raw`, the test driver has no processes), 「実行時エラー」 rows; vm.md 「操作の記録」 (`Wait` records, owner == host), 「救済」 (a task waiting on a `Wait` record is never rescued, so its owner does not move), 「上限と割り込み」 (Stream/Exec waits stop at the next safe point on every OS; the Windows sync-read limit stays for sync rows), 「時計と driver」 (pipes made by `pipe` on the spawning worker's driver, unbound until the first op; the virtual-clock guard); modules.md:69 list; testing.md 「テストの置き場所」 (real-clock UI tests for `Exec`); object-model.md T11 row (`launch` with pipes, `Process::raw`).

Tests:
- New UI with `-- clock: real (<reason>)` (≤ 3 `eml` children each, no real-time waits): `tests/ui/run/async/exec_pipeline.em` (3-stage `capture`), `exec_stdin_text.em` (`stdin_text` of 200 KiB through a `cat`-like child with `output`), `exec_child_released_on_abort.em`, `exec_wait_all.em` (`[3, 0]` from 2 children, then a third child: `Exec.output (Proc.stdout_file "out.txt" …)` gives `("", 0)` and the snapshot shows `--- written: out.txt ---`); `tests/ui/run-fail/async/exec_write_after_exit.em` (take `stdin`, `wait` the child that exits at once without reading, then `Stream.write` -> uncaught ``ConnectionReset``-kind ``cannot write to `eml`: broken pipe``; ruling.md Q2 item 5).
- New UI without the mark (virtual clock, seeds and N = 2/8): `tests/ui/run-fail/async/exec_needs_the_real_driver.em` (a started task calls `Exec.spawn`; ``cannot run `eml`: the test driver has no processes`` on every worker).
- cells.rs: `a_wait_record_is_hosted_by_its_owner`, `a_wait_on_a_host_without_processes_is_done_at_submit`, `a_dropped_exec_child_kills_and_waits_every_stage`, `a_pipe_write_broken_pipe_is_outcome_reset`, `a_read_of_a_pipe_grows_at_submit` (Miri set).
- `crates/eml_vm/tests/api.rs`: `a_child_pipe_is_adopted_by_another_worker` (N = 4), `a_wait_from_a_task_placed_on_worker_2_ends_under_rescue_always`, `a_cancelled_exec_wait_kills_the_child` (Clock::Real; ASan and TSan through `api::`).
- `crates/eml_cli/tests/cli.rs`: `an_interrupt_stops_a_program_waiting_in_exec_wait_with_130` (`#[cfg(unix)]`, as [V] cli.rs:882).
- Kind 2: `crates/eml_core_ir/tests/externs.rs` tables.

Verification:
- `git grep -n '^-- clock: real (' -- tests/ui` -> only `tests/ui/run/async/exec_*.em` and `tests/ui/run-fail/async/exec_*.em`.
- `files=$(git grep -l '^-- clock: real (' -- tests/ui); [ -z "$files" ] || git grep -ln "sleep_ms\|timeout\|_within" -- $files` prints nothing.
- `EML_UI_FILTER=async/exec_ cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'` -> 2 tests passed.
- `timeout 20 cargo run -p eml_cli -- run tests/ui/run/async/exec_pipeline.em` prints the snapshot's stdout and exits 0.
- `cargo nextest run -p eml_vm --lib -E 'test(cells::a_wait_record_is_hosted_by_its_owner) | test(cells::a_pipe_write_broken_pipe_is_outcome_reset)'` -> 2 passed.
- `cargo nextest run -p eml_vm --test integration -E 'test(api::a_child_pipe_is_adopted_by_another_worker)'` -> 1 passed.
- `git grep -n "Op::Job" -- crates/eml_vm/src/cells.rs` prints nothing; `git grep -n "Async.OpId" -- std/Exec.em` shows only non-`pub` `extern` rows; `git grep -n "op_prefix" -- std/Exec.em` shows one non-`pub` row and its use; `git grep -n "Async" -- std/Proc.em` prints nothing.
- `nix develop .#miri … cargo miri test -p eml_vm --lib -- cells::a_read cells::a_write counters::` both models -> pass.
- eml_rt gate (Verus, verusfmt, Miri `trusted::` both models).
- CI `cargo nextest run --profile timing -j 1`: `ui::run` and `ui::run_fail` under 10 s.

---

### Commit 12: Add the standard error pipe to Exec

Purpose: S1 A4's `PipeStderr`, `Exec.pipe_stderr`, `Exec.stderr`, and its launch errors (G-11).

Depends on: commit 11.

Files: `std/Prelude.em` (`PipeStderr`), `std/Proc.em` (`launch_args` stderr code 3; third argument enforced), `std/Exec.em` (`pipe_stderr`, `stderr`, `spawn` passes `True`), `crates/eml_extern/src/lib.rs` (`ExecStderr`), `crates/eml_rt/src/host.rs` (`StderrSpec::Given`, `PipeEnd::Stderr`, stderr code 3), `crates/eml_vm/src/{cells.rs,vm.rs,hosts.rs}` (`ChildPipe::Stderr`, `Prefix` ``cannot read the errors of `prog` ``, `ExecChild.stderr`), `crates/eml_core_ir/tests/externs.rs`.

Concrete change: `launch_args`'s `Bool` allows `PipeStderr` on the last stage only; otherwise `fail_io Other "cannot run `\{p}`: the standard error pipe needs `Exec.spawn` on the last stage"` (every `Proc` form and `Exec.run`/`output`/`capture` pass `False`). `pipe_stderr c = { c | stderr = PipeStderr }`.

Spec: stdlib.md 「`Cmd`」 (`PipeStderr`), 「`Std.Exec`」 (`pipe_stderr`, `stderr`), 「`Proc`」 (the launch error), 「実行時エラー」.

Tests (new): `tests/ui/run/async/exec_stderr_split.em` (mark; one child; two tasks read stdout and stderr; printed after both end, fixed order); `tests/ui/run-fail/async/exec_pipe_stderr_on_run.em` (mark not needed: no child starts; but it runs under seeds and N = 2/8 on the virtual clock, where the eml-side error comes before the driver guard); `tests/ui/run/io/try_io_pipe_stderr_on_proc.em` (`Proc.run (Exec.pipe_stderr …)` -> `Other: cannot run …: the standard error pipe needs `Exec.spawn` on the last stage`). Kind 2: `crates/eml_core_ir/tests/externs.rs`; the `Cmd` snapshots do not change (default `InheritStderr`).

Verification: `EML_UI_FILTER=pipe_stderr cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'` -> 2 tests passed; `EML_UI_FILTER=exec_stderr_split cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)'` -> 1 passed; eml_rt gate (Verus, verusfmt).

---

### Commit 13: Close async-files

Per docs/future/roadmap.md 「進め方」 items 1-9 (doc list in section 5 Q8). Measure with `bench/scripts.sh`, `bench/run.sh`, `bench/rt.sh` on the last code commit's tree; replace the three tables of benchmarks.md 「今の数値」 (or a following `Measure async-files` commit); delete `docs/plans/<date>-async-files.md`.

## 2. Rationale

- The test-Host store (1) and the Host interface refactor (2) each change one thing with no expected-value change, so the later feature commits only add behaviour.
- Sync features (3-4) settle `launch` and the spawn interface that `Exec` reuses and carry the only kind-2 `Cmd` snapshot change early.
- Each eml_rt `pub` item lands with its first implementation (files in 5, `open_raw` in 7, pipes and waits in 8, `Given`/`raw`/pipe callback in 11, stderr in 12); non-`pub` items land with their first reader.
- G-10 binds the T10 text to the first core `FileRead`: commit 5 carries both docs and both Miri test files.
- Test driver (6) before `Std.Stream` (7) so the stream UI tests run under seeds and N = 2/8 from their first commit.
- Unix (8) and Windows (9) split because Windows is checked only by CI; commit 8 keeps Windows compiling and states the interim refusal in vm.md.
- `Exec` is split into the process machinery (11) and the third pipe (12) to keep each reviewable.

## 3. Alternatives considered and rejected

- Middle-stage stdout validation in Rust with a 6-field stage encoding: encodes every stdout mode per stage only to refuse it (D1).
- Duplicating the `Cmd` encoder in `Exec.em`: two copies of the stage walk and the setter rule (D2).
- Lazy adoption of files on the I/O host by mail, or `bind_stream` reaching worker 0's driver: rejected with evidence in ruling.md Q1.
- A second Rust payload `FileStream`: duplicates six helpers or adds a trait (D4).
- Outcomes 0/6 with `finish` throwing everything: violates G-12 (ruling.md Q2).
- Two `CaptureHost` stores (memory + temp): cross-API reads need copying between stores (D7).
- Reaping a pipeline last stage first: does not remove the only deadlock (an unread last stdout) and reorders the captured stderr of `CaptureHost` (review C-5).

## 4. Assumptions made

- `deriving (Eq, Show)` on `StdinMode` (with `StdinFrom Cmd`) next to the hand-written `instance Show Cmd` compiles [I]. Impact if false: write `Show StdinMode` by hand.
- Linux `O_NONBLOCK = 0o4000`, `O_NOCTTY = 0o400` on x86_64 and aarch64 [I]; `api::a_stream_open_of_a_fifo_fails_at_once` catches a missing `O_NONBLOCK` on Linux CI.
- `libc` exposes `SYS_pidfd_open`, `waitid`, `P_PID`, `WNOWAIT` on both unix targets [I].
- `std::os::windows::fs::OpenOptionsExt::{access_mode, share_mode, custom_flags}` and `From<OwnedHandle> for Stdio` exist [I] (S1 §4).
- Windows maps a write to a pipe whose client closed to `ERROR_NO_DATA` or `ERROR_BROKEN_PIPE` [I]; iocp.rs maps both to `BrokenPipe`.
- The `ui::run` total stays under CI's 10 s with 7 stream and 5 exec run files (roadmap.md:66 reports 4.2-5.3 s today [I from R3]).
- `std::process::Child::id()` fits `i32` on unix [I].

## 5. Implications for implementation

### Q2: Extern rows

`kinded(name, &[arg reprs], &[arg marks], ret repr, ret mark, purity)`; Borrow = `borrowing(&[..], FunctionRow { with_arg: Some(i), .. })` (returns `(x, arg i)`); A = in `is_async()`. Commit in parentheses.

Type rows: `StreamReader => type_row("Std.Stream.Reader", Lin, Repr::Obj)`, `StreamWriter => type_row("Std.Stream.Writer", Lin, Repr::Obj)` (7), `ExecChild => type_row("Std.Exec.Child", Lin, Repr::Obj)` (11); each gets a probe in `crates/eml_core_ir/tests/externs.rs` `PROBES` and `probe()`.

| Row | eml type | Reprs -> ret | Marks -> ret | Borrow | Purity | A |
|---|---|---|---|---|---|---|
| `Std.Proc.spawn_raw` (3, changed) | `String -> Int -> String -> Int -> String -> <IO> Child` | `[Obj, Int, Obj, Int, Obj] -> Obj` | `[Unr; 5] -> Lin` | – | Effectful | no |
| `Std.Proc.stage_count` (4) | `Child -> (Int, Child)` | `[Obj] -> Int` | `[Lin] -> Unr` | `&[true]`, 0 | Pure | no |
| `Std.Proc.wait_stage` (4) | `Int -> Child -> <IO> (Int, Child)` | `[Int, Obj] -> Int` | `[Unr, Lin] -> Unr` | `&[false, true]`, 1 | Effectful | no |
| `Std.Stream.submit_open` (7) | `String -> Int -> <IO> Async.OpId` | `[Obj, Int] -> Int` | `[Unr, Unr] -> Unr` | – | Effectful | yes |
| `Std.Stream.submit_read` (7) | `Bytes -> Reader -> <IO> Async.OpId` | `[Obj, Obj] -> Int` | `[Unr, Lin] -> Unr` | – | Effectful | yes |
| `Std.Stream.submit_read_line` (7) | `Reader -> <IO> Async.OpId` | `[Obj] -> Int` | `[Lin] -> Unr` | – | Effectful | yes |
| `Std.Stream.submit_write` (7) | `Bytes -> Writer -> <IO> Async.OpId` | `[Obj, Obj] -> Int` | `[Unr, Lin] -> Unr` | – | Effectful | yes |
| `Std.Stream.op_reader` (7) | `Async.OpId -> <IO> (Reader, Async.OpId)` | `[Int] -> Obj` | `[Unr] -> Lin` | `&[true]`, 0 | Effectful | yes |
| `Std.Stream.op_writer` (7) | `Async.OpId -> <IO> (Writer, Async.OpId)` | `[Int] -> Obj` | `[Unr] -> Lin` | `&[true]`, 0 | Effectful | yes |
| `Std.Stream.op_outcome` (7) | `Async.OpId -> <IO> (Int, Async.OpId)` | `[Int] -> Int` | `[Unr] -> Unr` | `&[true]`, 0 | Effectful | yes |
| `Std.Stream.op_prefix` (7) | `Async.OpId -> <IO> (String, Async.OpId)` | `[Int] -> Obj` | `[Unr] -> Unr` | `&[true]`, 0 | Effectful | yes |
| `Std.Stream.take_reader` (7) | `Async.OpId -> <IO> Reader` | `[Int] -> Obj` | `[Unr] -> Lin` | – | Effectful | yes |
| `Std.Stream.take_writer` (7) | `Async.OpId -> <IO> Writer` | `[Int] -> Obj` | `[Unr] -> Lin` | – | Effectful | yes |
| `Std.Stream.take_bytes` (7) | `Async.OpId -> <IO> Bytes` | `[Int] -> Obj` | `[Unr] -> Unr` | – | Effectful | yes |
| `Std.Stream.take_line` (7) | `Async.OpId -> <IO> String` | `[Int] -> Obj` | `[Unr] -> Unr` | – | Effectful | yes |
| `Std.Stream.finish` (7) | `Async.OpId -> <IO> Unit` | `[Int] -> Unit` | `[Unr] -> Unr` | – | Effectful | yes |
| `Std.Stream.source` (7) | `Reader -> (String, Reader)` | `[Obj] -> Obj` | `[Lin] -> Unr` | `&[true]`, 0 | Pure | yes |
| `Std.Exec.spawn_raw` (11) | `String -> Int -> String -> Int -> String -> <IO> Child` | `[Obj, Int, Obj, Int, Obj] -> Obj` | `[Unr; 5] -> Lin` | – | Effectful | yes |
| `Std.Exec.stdin` pub (11) | `Child -> <IO> (Stream.Writer, Child)` | `[Obj] -> Obj` | `[Lin] -> Lin` | `&[true]`, 0 | Effectful | yes |
| `Std.Exec.stdout` pub (11) | `Child -> <IO> (Stream.Reader, Child)` | `[Obj] -> Obj` | `[Lin] -> Lin` | `&[true]`, 0 | Effectful | yes |
| `Std.Exec.stderr` pub (12) | `Child -> <IO> (Stream.Reader, Child)` | `[Obj] -> Obj` | `[Lin] -> Lin` | `&[true]`, 0 | Effectful | yes |
| `Std.Exec.stage_count` (11) | `Child -> (Int, Child)` | `[Obj] -> Int` | `[Lin] -> Unr` | `&[true]`, 0 | Pure | yes |
| `Std.Exec.submit_wait` (11) | `Int -> Child -> <IO> Async.OpId` | `[Int, Obj] -> Int` | `[Unr, Lin] -> Unr` | – | Effectful | yes |
| `Std.Exec.op_child` (11) | `Async.OpId -> <IO> (Child, Async.OpId)` | `[Int] -> Obj` | `[Unr] -> Lin` | `&[true]`, 0 | Effectful | yes |
| `Std.Exec.op_outcome` (11) | `Async.OpId -> <IO> (Int, Async.OpId)` | `[Int] -> Int` | `[Unr] -> Unr` | `&[true]`, 0 | Effectful | yes |
| `Std.Exec.op_prefix` (11) | `Async.OpId -> <IO> (String, Async.OpId)` | `[Int] -> Obj` | `[Unr] -> Unr` | `&[true]`, 0 | Effectful | yes |
| `Std.Exec.finish` (11) | `Async.OpId -> <IO> Unit` | `[Int] -> Unit` | `[Unr] -> Unr` | – | Effectful | yes |
| `Std.Exec.reap` (11) | `Int -> Child -> <IO> (Int, Child)` | `[Int, Obj] -> Int` | `[Unr, Lin] -> Unr` | `&[false, true]`, 1 | Effectful | yes |

Tie tests (kind 2, add rows; only `spawn_raw` changes an existing row): `crates/eml_core_ir/tests/externs.rs` `only_the_listed_rows_borrow_or_return_a_pair` gets every row with a Borrow entry, `only_the_listed_rows_carry_a_lin_or_poly_mark` every row with a `Lin` mark, and both doc comments' counts. The `eml_hir` std tie passes once each module declares exactly these `extern`s and `STD` lists the file. `Extern::is_async` doc comment: "…`Std.Atomic`、`Std.Net`、`Std.Stream`、`Std.Exec` の行".

### Q3: eml_vm cells.rs

- `Bind { Bound { host, handle, bin }, Unbound(RawResource), Lost }` (5). `bind_stream` unchanged rule (`here.host == here.me`); only sockets (accepted) and pipe ends (11) are ever unbound; files are bound at open (D3).
- `IoStream` (7), `Origin::Peer | File` (7), `Origin::Child` (11). `is_socket()` from `Origin::Peer`.
- Read rule (7): non-socket streams lend or grow at submit, phase `Transfer`; line chains grow at submit and in `LineStep::Again`; `Readable` only for sockets.
- Kinds: `OpKind::Open` (7; settle like `Connect`), `OpKind::Wait` (11); `Prefix` on `Read`/`Write`/`Line` (7: `Net`, `File`; 11: `Child`, `Wait`; 12: stderr). `cost()` of `Open`/`Wait` = 1. `outcome()` unchanged.
- `ExecChild` and `Drop` (11; `stderr` field in 12).
- `submit_wait`: owner = host = `here.me` (G-12); a task waiting on it is never rescued (vm.md 「救済」), so the owner stays.
- Dispatch: shared arms by function across `Net*`/`Stream*`/`Exec*` (`op_stream`, `op_outcome`, `op_prefix` (Stream/Exec only), `take_bytes`, `take_line`, `finish`); per-module arms for submits, `take_reader`/`take_writer`, `source`, `spawn_raw`, takers, `stage_count`, `reap`.

### Q4: eml_rt gate per commit

| Commit | eml_rt files | Verus + verusfmt | Miri `trusted::` (SB + TB) | trust list |
|---|---|---|---|---|
| 2 | host.rs, lib.rs, heap/tests/host.rs | yes | yes (Host boundary of trusted child tests) | T11 text unchanged in meaning; object-model.md 「子のプロセス」 |
| 3 | host.rs, externs.rs, heap/ctx/proc.rs (3-word proof), lib.rs, heap/tests/* | yes | yes | T11 row text; src/trust.rs unchanged |
| 4 | host.rs, heap/ctx/proc.rs, trust.rs, heap/tests/* | yes | yes (new pipeline test) | T11 text; src/trust.rs + `child_wait_stage`, `child_stage_count` |
| 5 | driver.rs, lib.rs, heap/tests/trusted.rs | yes | yes, 280 s / 365 s; new test ≤ 5 s TB | T10 row |
| 7 | host.rs (`open_raw`), lib.rs, heap/tests/host.rs | yes | yes (host.rs changed) | T11 text |
| 8 | driver.rs, lib.rs | yes | not required (no trusted change) | – |
| 11 | host.rs (`Given`, `pipes`, `raw`), lib.rs | yes | yes | T11 text |
| 12 | host.rs (stderr pipe) | yes | yes | – |

Each also passes `cargo nextest run`, clippy and `cargo fmt` (crates/eml_rt/CLAUDE.md).

### SPAWN_LOCK (D10)

- Rule (object-model.md 「子のプロセス」, commit 3): `SPAWN_LOCK` is a leaf lock. It is held only (a) by `IoDriver::pipe` around creating one pipe pair and setting its flags, and (b) by `Host::spawn` from before the first here-file, redirect file or inter-stage pipe until the last `Command::spawn` returns. While it is held, no other eml lock is taken, no driver or Host callback runs, no thread is created, and output is not flushed (hosts flush before taking it; `CaptureHost` starts drain threads after releasing it). `launch` never holds it, so its `pipes` callback (which calls `IoDriver::pipe`) and `Host::spawn` take it one after the other, never nested.
- Why the gap between `pipe` and `spawn` is safe: every descriptor eml makes is close-on-exec or non-inheritable from the moment it exists outside the lock: Linux `pipe2(O_CLOEXEC)` and std's files (`O_CLOEXEC`) are atomic; on Apple, `pipe` + `FD_CLOEXEC` happens inside the lock, so no eml spawn forks in between; Windows named-pipe ends and std's files are created with null security attributes (not inheritable). A concurrent spawn therefore cannot inherit a `Given` end created before its own lock section; std gives each child its own ends by `dup2` (unix) or by duplicating them inheritable under `CREATE_PROCESS_LOCK` (Windows [V] R4 §3).

### Q5: eml_io

- Core: `Class` per handle (5: Socket/File; 8: Pipe); `Kind::{FileRead, FileWrite}` (5), `Kind::Wait` (8); `Step::Wait` (8); `Backend::{file_job (5), pipe (8)}`; `Adopt` at submit (5); `Drop` waits `Memory`-held jobs (5); `cancel` no-op on file jobs. OS-free (trust.rs checks).
- `sys/unix.rs`: `file_job` (5); pipes, class-based read/write, pidfd, `EVFILT_PROC` + `waitid(WNOWAIT)` (8).
- `sys/iocp.rs`: `file_job` (5); named pipes, `PIPE_KEY`, `What::PipeRead/PipeWrite`, `Exited` (9).
- Cargo.toml windows-sys: + `Win32_Storage_FileSystem` (5); + `Win32_System_Pipes`, `Win32_Security` (9).
- `src/trust.rs`: no new source file; `FILES`/`UNSAFE_FILES`/`COMPLETION_BACKENDS` unchanged.
- Test driver: files (6); pipe/wait refusals (8).

### Q6: UI harness

Commit 10: `leading_marks`/`reason_mark` ([V] common/mod.rs:176-245), `extra_runs` gate in ui.rs (`run`, `run_fail_file`), `execute_in` `Clock::Real` ([V] ui.rs:179-215). Marked files run checked + fast only.

### Q7: Test-change list

Kind 1: none.

Kind 2 (scopes):
1. Commit 3: snapshots printing a `Cmd`: `git grep -ln "stderr = InheritStderr" -- crates/eml_cli/tests/snapshots` -> `integration__ui__run@basics__command_literal.em.snap`, `integration__repl__transcripts@std__command_literal.repl.snap` [V]; they gain `stdin = DefaultStdin, stdout = DefaultStdout`. Reason: `Cmd` gains fields.
2. Commits 3, 4, 7, 11, 12: `crates/eml_core_ir/tests/externs.rs` tables and doc-comment counts. Reason: the table pins every row.
3. `bench/` `RunStats` inline snapshots in `crates/eml_vm/tests/bench.rs`: checked in commit 3's verification; expected unchanged, any change is accepted as kind 2 with the reason "Prelude grew".
4. `crates/eml_rt/src/heap/tests/expected.txt`: `grep -n "Std.Proc" crates/eml_rt/src/heap/tests/expected.txt` prints nothing [V]; expected unchanged.

Kind 3 (mechanical, byte-identical expectations): commit 2 Host-impl call shapes (`crates/eml_rt/src/heap/tests/host.rs`, `crates/eml_vm/tests/preempt.rs`, hosts.rs); commit 3 `proc_spawn` callers (`crates/eml_rt/src/heap/tests/{ctx.rs,script.rs,trusted.rs}`); commit 5 `RawResource::Socket` wrapping (`crates/eml_io/tests/real.rs`, `crates/eml_io/src/driver/tests/*`, cells.rs tests); commit 7 `NetStream` -> `IoStream` in cells.rs tests.

No test change at all (behaviour rewrite checked by unchanged snapshots): commit 1 (D7).

### Q8: Docs per commit

| Commit | docs |
|---|---|
| 1 | testing.md 「UI テスト」 (temp store, leaks on abort) |
| 2 | object-model.md 「子のプロセス」 (what `Host::spawn` receives) |
| 3 | stdlib.md 「`Cmd`」「`Proc`」「実行時エラー」; expressions.md 「コマンドリテラル」; object-model.md 「子のプロセス」 (`SPAWN_LOCK` rule), `Fault` table, T11; lir.md:835 |
| 4 | stdlib.md 「`Proc`」「`Cmd`」; object-model.md 「子のプロセス」, T11 |
| 5 | object-model.md T10; vm.md 「時計と driver」 (`RawBuf`, `Job`, `RawResource`/`RawFile`, `Adopt`, abort reachability); testing.md 「ThreadSanitizer」「核の Miri」「よく使うコマンド」; ci.yml; CLAUDE.md TSan command |
| 6 | vm.md 「時計と driver」 (test driver files) |
| 7 | stdlib.md 「`Std.Stream`」「実行時エラー」; vm.md 「操作の記録」「時計と driver」 (open, host fixed at open, rescue); object-model.md T11; modules.md:69; async.md:261, vm.md:523, roadmap.md 「プールの仕事の予算と、放さずに終わる方針」 and `ffi-blocking`; status.md:147 |
| 8 | vm.md 「時計と driver」 (pipes, waits, unix backends, IOCP interim refusal, test-driver refusals) |
| 9 | vm.md 「時計と driver」 (IOCP) |
| 10 | testing.md 「UI テスト」「seed で流す」 |
| 11 | stdlib.md 「`Std.Exec`」「実行時エラー」; vm.md 「操作の記録」「救済」「上限と割り込み」「時計と driver」; object-model.md T11; modules.md list; testing.md 「テストの置き場所」 |
| 12 | stdlib.md 「`Cmd`」「`Std.Exec`」「`Proc`」「実行時エラー」 |
| 13 (close) | 1. spec sweep (`git grep -n "async-files" -- docs/spec docs/implementation` prints nothing; 「今は〜」 forecasts read by hand). 2. benchmarks.md 「今の数値」 three tables. 3. status.md: :31 `Proc` scope, :56 remove pipelines, stdin text, `Child` stderr pipe and redirection (keep 「標準エラーを別の文字列に取ること」 for `Proc` and 「環境変数の削除」), :147, :179-185 (sync limits stay; new limits: `Stream` regular files only, no seek, Linux < 5.3). 4. architecture.md eml_io section (pipes, files, waits, file jobs; no crate or pass change). 5. roadmap.md: delete the `async-files` entry (:31-34); add to 「その後の項目」 with 後に回した理由 / 見直す条件: buffered async `Writer`, `_within` on `Stream`, `Exec.output_err`, per-stage stderr readers, async `Fs`, Linux < 5.3 `Exec.wait`, `env` on every stage, seek / read+write mode, async `Stdin`/`Stdout`; remove 「標準入力と標準出力のファイルへのリダイレクト」 (:167); rephrase :170-172 for `Proc` only (`Exec` covers it); replace `async-files` in `jit-effects`'s 前提 (:58) by the item names; add the item 「Windows で子の pipe を読むタスクの Ctrl-C を確かめるテスト」 with 後に回した理由: `crates/eml_cli/tests/cli.rs` にコンソールのイベントを送る仕組みがない。 and 見直す条件: Windows のコンソールの Ctrl-C を送る仕組みを cli.rs に足すとき、か、Windows で `Std.Exec` の待ちが Ctrl-C で止まらない報告が出たとき。 async.md 「ファイルとパイプ」: keep only what is future, cite the spec sections. 6. overview.md decision table row; README.md 「文書の地図」 only if a 内容 cell changed. 7. CLAUDE.md: Topics row for `Std.Stream`/`Std.Exec`; the driver-handle invariant (pipes adopted by the first submitting worker, files bound at open on the opener's I/O host); eml_io line. 8. delete the plan. 9. commit message per item 9. |

### Q9: S1 "How to check" commands, corrected

| G | Problem | Command the plan uses |
|---|---|---|
| G-1 | `git grep -n "<IO>"` also prints non-`pub` helpers (`raise_cancelled`, `fail_code`); the first check fails if `Proc.em` says "Async" (review C-1) | `git grep -n "Async" -- std/FileIn.em std/FileOut.em std/Fs.em std/Proc.em std/Stdin.em std/Stdout.em std/Stderr.em` prints nothing (D2 wording); `git grep -n "^pub .*<IO>" -- std/Stream.em std/Exec.em` prints only `close`, `close_writer`, `stdin`, `stdout`, `stderr` |
| G-2 | `test(ui::run)` is a substring match (also `ui::run_fail`); the repo's hint uses `=` [V] ui.rs:396; `EML_UI_FILTER` exists [V] common/mod.rs:72-75 | `EML_UI_FILTER=proc_pipeline cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)'` -> 1 test passed; grep as written |
| G-3 | same | `EML_UI_FILTER=proc_stdin_text … -E 'test(=ui::run)'`; greps as written |
| G-4 | `WaitForSingleObject` matches `RegisterWaitForSingleObject` | `git grep -nw "waitpid\|waitid\|WaitForSingleObject" -- crates/eml_io/src`; nextest command as written |
| G-5 | the grep prints `src/trust.rs:250` today [V] | `grep -rn "from_raw_parts" crates/eml_io/src \| grep -v "src/test.rs\|src/trust.rs"` prints nothing |
| G-6 | `test(ui::run)` | `EML_UI_FILTER=stream_open_special … -E 'test(=ui::run)'`; grep as written |
| G-7 | amended by ruling.md Q1 | as written, plus `cargo nextest run -p eml_vm --lib -E 'test(cells::an_open_record_binds_the_file_to_the_openers_host)'` |
| G-8 | none | as written |
| G-9 | none | as written |
| G-10 | none | as written; the two Miri commands = `trusted::` and `eml_io --lib -- driver::tests::`, each under `-Zmiri-strict-provenance` and again with `-Zmiri-tree-borrows` |
| G-11 | `test(ui::run)`, `test(ui::run_fail)` | `EML_UI_FILTER=proc_pipeline … 'test(=ui::run)'`; `EML_UI_FILTER=proc_stage_redirect … 'test(=ui::run_fail)'`, snapshot names `eml-first` |
| G-12 | ruling.md Q2 completion criteria added | as written, plus `git grep -n "op_failed" -- std/` prints nothing, `git grep -n "op_prefix" -- std/Stream.em std/Exec.em` one non-`pub` row per module and its uses, `git grep -n "OUTCOME_FAILED" -- crates/eml_vm/src/cells.rs` shows no class-conditional branch in `outcome()`; `cells::a_wait_record_is_hosted_by_its_owner` resolves (cells.rs tests are top-level `#[cfg(test)]` fns [V] cells.rs:5671) |

## 5b. Verification commands per criterion

The working commands at the stage-close commit (design.md section 8 with S2's corrections). Each must give the stated result on a clean tree.

| G | Command | Expected |
|---|---|---|
| G-1 | `git grep -n "Async" -- std/FileIn.em std/FileOut.em std/Fs.em std/Proc.em std/Stdin.em std/Stdout.em std/Stderr.em` | prints nothing |
| G-1 | `git grep -n "^pub .*<IO>" -- std/Stream.em std/Exec.em` | only the `close`, `close_writer`, `stdin`, `stdout`, `stderr` lines |
| G-2 | `git grep -n "SPAWN_LOCK" -- crates/eml_rt/src/host.rs crates/eml_vm/src/hosts.rs crates/eml_io/src/sys` | a `.lock()` in each `spawn` (host.rs, hosts.rs) and in each `pipe` (unix.rs, iocp.rs), plus the definition |
| G-2 | `EML_UI_FILTER=proc_pipeline cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)'` | 1 test passed |
| G-3 | `git grep -n "windows_sys\|libc::" -- crates/eml_rt/src/host.rs`; `git grep -n "thread::spawn\|spawn_masked" -- crates/eml_rt/src/host.rs` | both print nothing |
| G-3 | `EML_UI_FILTER=proc_stdin_text cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)'` | 1 test passed (2 MiB text) |
| G-4 | `git grep -nw "waitpid\|waitid\|WaitForSingleObject" -- crates/eml_io/src` | only `waitid(… WNOWAIT …)` in sys/unix.rs and its test |
| G-4 | `git grep -n "Op::Job" -- crates/eml_vm/src/cells.rs` | prints nothing |
| G-4 | `cargo nextest run -p eml_io --test integration -E 'test(real::a_wait_on_a_running_child_is_cancelled_at_once)'` | 1 passed |
| G-5 | `grep -rn "from_raw_parts" crates/eml_io/src \| grep -v "src/test.rs\|src/trust.rs"` | prints nothing |
| G-5 | `cargo nextest run -p eml_io --lib -E 'test(driver::tests::a_dropped_driver_waits_for_a_file_job)'` | 1 passed |
| G-5 | `git grep -n "Op::Readable" -- crates/eml_vm/src/cells.rs` | every line inside a socket-class arm (read by hand) |
| G-6 | `git grep -n "O_NONBLOCK" -- crates/eml_rt/src/host.rs` | the open-flag constants and their use in `open_raw` |
| G-6 | `EML_UI_FILTER=stream_open_special cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)'` | 1 test passed |
| G-7 | `cargo nextest run -p eml_io --lib -E 'test(sys::unix::tests::a_pipe_parent_end_is_nonblocking_and_the_child_end_is_not)'` (unix) | 1 passed |
| G-7 | `cargo nextest run -p eml_io --lib -E 'test(sys::iocp::tests::a_pipe_child_end_is_not_inheritable)'` (Windows CI) | 1 passed |
| G-7 | `cargo nextest run -p eml_vm --test integration -E 'test(api::a_child_pipe_is_adopted_by_another_worker)'` | 1 passed |
| G-7 | `cargo nextest run -p eml_vm --lib -E 'test(cells::an_open_record_binds_the_file_to_the_openers_host)'` | 1 passed |
| G-8 | `cargo nextest run -p eml_io --test integration -E 'test(test_driver::a_cancelled_file_read_still_delivers_its_bytes) \| test(test_driver::the_test_driver_has_no_pipes)'` | 2 passed |
| G-8 | `git grep -n '^-- clock: real (' -- tests/ui` | only `tests/ui/run/async/exec_*.em` and `tests/ui/run-fail/async/exec_*.em` |
| G-8 | `files=$(git grep -l '^-- clock: real (' -- tests/ui); [ -z "$files" ] \|\| git grep -ln "sleep_ms\|timeout\|_within" -- $files` | prints nothing |
| G-9 | `cargo nextest run -p eml_cli --test integration -E 'test(ui::)' && git status --porcelain -- tests/` | tests pass; status prints nothing |
| G-9 | `grep -n "written: " crates/eml_cli/tests/snapshots/integration__ui__run@async__stream_write_read_back.em.snap` | at least one line |
| G-10 | `git log --format=%H -S"FileRead" -- crates/eml_io/src/driver/mod.rs \| tail -1 \| xargs git show --stat` | lists `docs/spec/object-model.md`, `docs/spec/vm.md`, `crates/eml_rt/src/heap/tests/trusted.rs`, `crates/eml_io/src/driver/tests/mod.rs` |
| G-10 | `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` and the same with `-Zmiri-strict-provenance -Zmiri-tree-borrows`; `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_io --lib -- driver::tests::` and the same with `-Zmiri-tree-borrows` | all pass |
| G-11 | `EML_UI_FILTER=proc_pipeline cargo nextest run -p eml_cli --test integration -E 'test(=ui::run)'` | 1 test passed; the snapshot shows the last code and `wait_all` = `[3, 0]` |
| G-11 | `EML_UI_FILTER=proc_stage_redirect cargo nextest run -p eml_cli --test integration -E 'test(=ui::run_fail)'` | 1 test passed; snapshot contains ``cannot run `eml-first`: the standard output of a stage that is not last is redirected`` |
| G-12 | `git grep -n "Async.OpId" -- std/Stream.em std/Exec.em` | only non-`pub` `extern` rows |
| G-12 | `git grep -n "op_failed" -- std/`; `git grep -n "op_prefix" -- std/Stream.em std/Exec.em` | first prints nothing; second shows one non-`pub` `extern` row per module and its uses in `Woke` branches |
| G-12 | `git grep -n "OUTCOME_FAILED" -- crates/eml_vm/src/cells.rs` | no class-conditional branch in `outcome()` (read by hand) |
| G-12 | `cargo nextest run -p eml_vm --lib -E 'test(cells::a_wait_record_is_hosted_by_its_owner)'` | 1 passed |

## 6. Unresolved items with defaults

- `ui::run` total time: default the listed files only; if CI's timing job exceeds 10 s, move `exec_child_released_on_abort.em` to `api.rs` (the deferred item 「`ui::run` の実時間を5秒の条件に収めること」 applies).
- A redirected stdout under `output`/`capture` of `Proc` and `Exec`: nothing is read; the result is `""` (or empty `Bytes`), documented in stdlib.md 「`Proc`」 and 「`Std.Exec`」.
- `Exec.wait` on Linux < 5.3: `NotAvailable` from `ENOSYS`; roadmap item.
- `env` per stage: last stage only (S1 §6).
- Windows Ctrl-C while a task reads a child pipe: no Windows console-event harness in cli.rs ([V] no `GenerateConsoleCtrlEvent`); unix test only; the close commit adds a roadmap deferred item (Q8 row 13).

## 7. Additional checks

- r1 checks stand (see r1 §7): `EML_UI_FILTER`, `Fault`/`IoReason`, host.rs spawn path, `CaptureHost`, `bind_stream`, read submit and probe path, extern rows and tie tables, `eml_hir::STD`, modules.md:85, stdlib.md:1046, macOS header values, windows-sys feature names.
- r2:
  - quiesce before every driver drop: `retire_driver` (workers.rs:1250-1262), worker 0 after join on success/error/exit (lib.rs:1044-1060), `recover` (lib.rs:511-518), `Machine::drop` (lib.rs:832-843, leaks the heap when poisoned); `quiesce` returns early only when broken (driver/mod.rs:823-873) [V].
  - `outcome()` maps `BrokenPipe` to `OUTCOME_RESET`; `Taken::failure()` uses `kind.prefix()` (cells.rs:496-549) [V].
  - `Connect` settle builds `made` without reading `cancelling` (cells.rs:3433-3452) [V].
  - the existing pipe rule 「`wait` の前に標準出力を読み切る」 (stdlib.md:729-748) [V].
  - `CaptureHost` users across crates (`git grep -ln "CaptureHost\|capture_host\|script_host" -- crates`: eml_cli, eml_test_support, eml_vm tests) [V]; only `ui.rs` reads `written()` [V].
  - `directory_error` (host.rs:1416-1425) [V].
- No builds were run; all checks are source reads.

## 8. Challenges to the guideline

- **G-7 (files)**: resolved by the approved amendment (ruling.md Q1); D3 conforms.

