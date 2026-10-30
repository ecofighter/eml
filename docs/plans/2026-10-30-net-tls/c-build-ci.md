# 付録 C: ビルド、feature、CI、nix

> 付録。本文 (`../2026-10-30-net-tls.md`) の決定が優先する。食い違いは本文の「付録との差分」に挙げた。ここに出てくる [V] は設計の段階で試作して確かめたこと、[I] は推定である。



## 1. Conclusion

**Dependencies.** Add these to the workspace:
- `rustls = { version = "0.23.46", default-features = false, features = ["aws_lc_rs", "std", "tls12"] }`
- `webpki-roots = "1.0.11"`

No `logging`, no `prefer-post-quantum`, and no separate `rustls-pki-types`: `rustls::pki_types::pem::PemObject` is available.

**Windows.** rustls `aws_lc_rs` turns on `aws-lc-rs/prebuilt-nasm`, which turns on `aws-lc-sys/prebuilt-nasm`. aws-lc-sys uses NASM if it is present, and the prebuilt objects otherwise. ci.yml needs no Windows step.

**nix.**
- `flake.nix` does not change.
- `nix build`'s checkPhase runs `cargo test` over the whole workspace.
- With T1 applied, it compiled aws-lc-sys and passed the link test in the sandbox.

**Features.**
- `eml_io`: `tls = ["dep:eml_tls"]`.
- `eml_vm`: `tls = ["dep:eml_tls", "eml_io/tls"]`.
- `eml_cli`: `tls = ["run", "eml_vm/tls"]` and `default = ["run", "tls"]`.
- `eml_test_support` gets no `tls` feature, and no dev-dependency changes.
- Selecting `-p eml_vm`, `-p eml_io` or `-p eml_rt` (as the Miri and ASan jobs do) does not reach `eml_tls`.
- The workspace `cargo nextest run` and `nix build` turn tls on through unification.

**Dev-dependency rule (stage-wide).**
- Only `eml_tls` may have a dev-dependency that builds C crypto (`eml_tls`, `rustls`, `rcgen`, `ring`, `aws-lc-*`).
- Other crates reach `eml_tls` through their optional normal dependency, behind `#[cfg(feature = "tls")]`.
- A CI line enforces this rule (T2), and testing.md 「OS ごとのテスト」 states it.

**tls-off behaviour.**
- It is tested in `mod tls_off` of the `eml_io` and `eml_vm` integration tests, declared `#[cfg(not(feature = "tls"))]` and not OS-gated.
- Command: `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'`.
- Running the `eml_cli` suite with tls off is not supported, and testing.md says so.

**`.#cross` clippy** (all three targets):

```
nix develop .#cross -c cargo clippy --target <T> --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings
```

**CI changes.** The `test` job gets:
- two more lines in the feature-combo clippy step;
- a feature-resolution step with three `cargo tree` assertions;
- a tls-off nextest step.

The miri, asan, verus and fmt jobs do not change.

## 2. Rationale (facts)

**rustls and aws-lc features**
- [V: `rustls-0.23.46/Cargo.toml` in the registry]
  - `aws_lc_rs = ["dep:aws-lc-rs", "webpki/aws-lc-rs", "aws-lc-rs/aws-lc-sys", "aws-lc-rs/prebuilt-nasm"]`
  - `std = ["webpki/std", "pki-types/std", "once_cell/std"]`
  - The default features add `logging` and `prefer-post-quantum`.
- [V: `aws-lc-rs-1.19.0/Cargo.toml:88`] `prebuilt-nasm = ["aws-lc-sys?/prebuilt-nasm"]`.
- [V: `aws-lc-sys-0.47.0/builder/main.rs:1155-1164`] `use_prebuilt_nasm()` is true when all of these hold:
  - the target is Windows x86_64;
  - `nasm -version` fails;
  - the environment does not forbid it;
  - `AWS_LC_SYS_PREBUILT_NASM=1` or the feature is set.
- [V: `main.rs:783`, `:863`] The cmake path for sanitizers is taken only when the env var `AWS_LC_SYS_SANITIZER` is set. RUSTFLAGS has no effect on it.

**Feature isolation**
- [V: `cargo tree -e normal,dev` in `repo2`] Count of lines matching `eml_tls|aws-lc-sys`:

  | Selection | Count |
  |---|---|
  | `-p eml_vm` | 0 |
  | `-p eml_io` | 0 |
  | `-p eml_rt` | 0 |
  | `-p eml_cli --no-default-features --features run` | 0 |
  | `-p eml_cli` | 3 |
  | `--workspace` | 4 |

- [V: `repo2`] `cargo tree --workspace -e features -i eml_vm | grep -c 'eml_vm feature "tls"'` gives 1.
- [V: `repo2`] `cargo tree --workspace --exclude eml_tls --no-default-features --features eml_cli/run,eml_test_support/run -e normal,dev,build --target all | grep -c -E 'aws-lc|eml_tls'` gives 0.
  - With a test-only dev-dependency `eml_tls.workspace = true` added to eml_io, it gives 4. The guard catches the forbidden case.

**T1 build**
- [V: `cargo test -p eml_tls` in `repo`] The link test passes in 0.05 s. The first build took 39 s. Clippy is clean.
- [V: compiled a scratch test] `CertificateDer::pem_slice_iter` works with these features.
- [V: `nix build -L "git+file://…/exp-S3/repo"`, exit 0, log `nixbuild.log`]
  - Build phase: 2 min 51 s.
  - checkPhase compile: 4 min 09 s, then all workspace tests ran.
  - `cargoCheckHook flags: -j 4 --profile release --target x86_64-unknown-linux-gnu --offline -- --skip cli::a_script_with_a_shebang_runs_directly` (no `-p`).
  - The checkPhase compiled `aws-lc-sys v0.47.0` and passed `a_client_hello_comes_out_of_aws_lc`.
  - No change to `flake.nix`.

**Cross clippy**
- [V: `cross-win-full.log`] The current `.#cross` Windows command fails once `eml_tls` exists: `failed to run custom build command for aws-lc-sys v0.47.0`, exit 101.
- [V: `cross-*.log`] The new form passes for x86_64-pc-windows-msvc (1 min 42 s), aarch64-unknown-linux-gnu and x86_64-unknown-linux-gnu.

**CI**
- [V: in `repo2`] Both new clippy lines pass.
- [V: `nextest-empty.log`] When the filter matches nothing, nextest exits 4 (`no tests to run`).
- [V] actionlint passes on the edited ci.yml.
- [V: `/home/user/eml/crates/eml_cli/Cargo.toml`, `/home/user/eml/crates/eml_test_support/Cargo.toml`] Both crates already use `[lib] test = false` with the comment that you remove the line when you add unit tests.

## 3. Alternatives rejected

- **`eml_test_support` gets `tls`, or a dev-dependency enables tls.** This would pull aws-lc into the Miri and ASan jobs. Those jobs do not instrument C, and the `.#miri` shell has no cmake for an ASan build of aws-lc.
- **`.#cross` with `--exclude eml_io --exclude eml_vm`, or `--lib --bins` only.** The cross clippy exists to lint eml_io's `sys/` Windows backend and the `cfg(windows)` and `cfg(unix)` code in tests. Either alternative drops exactly that code.
- **`required-features = ["tls"]` on eml_cli's integration test.** The cross clippy would then stop linting eml_cli's test code.
- **Running the whole eml_cli suite with tls off.** The `run/tls` UI tests would fail.
- **An env var or tool step for one failing OS.** The design premise is "the C toolchain only". Adding NASM, cmake or an env var would change that premise, so the decision rule goes to ring instead.
- **Features `logging` and `prefer-post-quantum`, or cmake, nasm or clang in `flake.nix`.** None is needed.

## 4. Assumptions (impact if false)

- **A1. S2's tls-off tests.** S2 puts the off-behaviour tests in `mod tls_off` in eml_io (`tls.localhost` gives NotFound) and in eml_vm (`Std.Tls` gives NotAvailable).
  - Each module is `#[cfg(not(feature = "tls"))]` only, never OS-gated.
  - The `test(x)` filter is a substring match on `module::test`. No test outside these modules may contain `tls_off::` in its path.
  - If S2 uses other names, change the filter to match. If only one crate has the module, drop the other `-p`.
- **A2. S1/S2 tests that need tls.** They use `#[cfg(feature = "tls")]` and the optional dependency. Otherwise the T2 guard fails, which is intended.
- **A3. eml_cli tests that need `eml_tls` or `rustls`** (S2's real-socket test). Add `eml_tls = { workspace = true, optional = true }`, and `rustls` the same way if it is used directly, to `[dependencies]`, and put `"dep:eml_tls"` (and `"dep:rustls"`) in `eml_cli`'s `tls`. This adds nothing to the default build, which already links them through eml_vm.
- **A4. Other runners.** windows-2025, macos-15 and ubuntu-24.04-arm build aws-lc-sys with their default compilers [I]. The T1 CI gate verifies this.
- **A5. Nix on aarch64-linux.** It uses the same gcc stdenv as x86_64-linux, and the cargo build is covered by the CI arm runner [I]. It is not gated.

## 5. Implications for implementation

### Ordering rules (positions are settled at consolidation)

- **R1.** T1 is the first code commit of the stage. Both T1 gates (CI and darwin nix) must pass before any other commit of the stage that adds code using `eml_tls`.
- **R2.** The commit that declares `tls` in a crate's `[features]` (T2) must come before, or be the same as, the first commit with `cfg(feature = "tls")` or `cfg(not(feature = "tls"))` in that crate. An undeclared feature in a cfg triggers `unexpected_cfgs`, which fails `-D warnings`.
- **R3.** The CI step `cargo nextest run without tls` (T3) lands in the same commit as the first `tls_off` test module. It must not land earlier, because nextest exits 4 when no test matches.
- **R4.** The T2 guard line lands with T2 or earlier, so it is in force before S1/S2 add any dependency.

Every task ends with `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` both at exit 0. All Japanese text is written under the natural-japanese skill. The prose lists below are content points, not wording.

### T1: `eml_tls` skeleton that links aws-lc (CI gate and darwin nix gate)

**Write first:** `crates/eml_tls/tests/provider.rs`.

```rust
use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore};

// aws-lc の C のコードが組まれてリンクされ、鍵の共有と乱数で実際に呼ばれることを確かめる
#[test]
fn a_client_hello_comes_out_of_aws_lc() {
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let name = ServerName::try_from("tls.localhost").unwrap();
    let mut conn = ClientConnection::new(Arc::new(config), name).unwrap();
    let mut out = Vec::new();
    conn.write_tls(&mut out).unwrap();
    assert_eq!(out[0], 0x16); // handshake のレコード
    assert!(conn.is_handshaking());
}
```

The test is kept for the rest of the stage.

**Files**

- **`Cargo.toml` (workspace):**
  - After `eml_io = { path = "crates/eml_io" }`, add `eml_tls = { path = "crates/eml_tls" }`.
  - After `rustc-hash = "2.1"`, add the line below, with a comment: the only provider is aws-lc-rs, and `eml_tls` passes it with `builder_with_provider`. The defaults are off because `logging` and `prefer-post-quantum` are unused. Cite architecture.md 「crate の依存関係」.

    ```toml
    rustls = { version = "0.23.46", default-features = false, features = ["aws_lc_rs", "std", "tls12"] }
    ```
  - After `text-size = "1.1"`, add `webpki-roots = "1.0.11"`.
- **`Cargo.lock`:** regenerate it with cargo (about 280 lines change).
- **`crates/eml_tls/Cargo.toml`:**

  ```toml
  [package]
  name = "eml_tls"
  version.workspace = true
  edition.workspace = true
  license.workspace = true
  # 結合テストは1つのバイナリにまとめる (docs/implementation/testing.md の「crate の中の置き方」)
  autotests = false

  # 単体テストがないので、空のテストのバイナリを作らない。単体テストを足すときは `test = false` を外す
  [lib]
  test = false
  doctest = false

  [dependencies]
  rustls.workspace = true
  webpki-roots.workspace = true

  [[test]]
  name = "integration"
  path = "tests/main.rs"
  ```

  This follows the same convention as eml_cli and eml_test_support. S1's first unit test in `src/` deletes `test = false` and its comment as a mechanical follow-up.
- **`crates/eml_tls/src/lib.rs`:** only the crate doc comment. Content points: it wraps the rustls client connection without I/O, and it depends on no eml crate.
- **`crates/eml_tls/tests/main.rs`:** `mod provider;`.
- **Cross command.** It must change in this commit, because the current one fails as soon as `eml_tls` exists.
  - `CLAUDE.md`, Commands line 38:

    ```
    nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings   # type-check the Windows-only code; eml_tls (C crypto) is excluded (Linux: x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, same flags)
    ```
  - `docs/implementation/testing.md` 「よく使うコマンド」 line 642: the same command with a Japanese comment.
  - `docs/implementation/testing.md` 「OS ごとのテスト」, the paragraph at line 480. Content points:
    - `eml_tls` builds aws-lc-sys, and that needs the target's C toolchain. The `.#cross` shell lacks it, so that clippy run excludes `eml_tls` and turns `tls` off. The Linux targets use the same flags.
    - The tls-off code is linted by the feature-combo clippy lines.
    - `eml_tls` itself is linted and tested on every CI OS.
    - Only `eml_tls` may dev-depend on a crate that builds C crypto. Other crates' tests reach `eml_tls` through the optional dependency under `cfg(feature = "tls")`. CI checks this with `cargo tree` (from T2 on).
- **Crate lists:**
  - `CLAUDE.md` Architecture block: add `eml_tls          rustls client connection, sans-IO (aws-lc-rs provider, webpki-roots); depends on no eml crate` between `eml_io` and `eml_rt`.
  - `docs/implementation/architecture.md` 「crate の依存関係」: add the same line in Japanese, and a bullet with these points:
    - `eml_tls` depends on no workspace crate.
    - It uses rustls with aws-lc-rs and webpki-roots.
    - aws-lc-sys needs only the C toolchain. On Windows x86_64 it uses NASM, or the prebuilt objects if NASM is missing.
    - That is why `.#cross` excludes it.
  - `README.md` crate table: add `` | `eml_tls` | rustls の接続を包む TLS の状態 (I/O を持たない) | `` after `eml_rt`.
- `flake.nix` does not change. The reading map in docs/README.md is S1's.

**Test-change kind:** none.

**Local completion**
1. `cargo nextest run -p eml_tls` prints `1 test run: 1 passed`.
2. `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` exit 0.
3. The `.#cross` command exits 0 for `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`.
4. `nix build -L` exits 0, and its log contains `Compiling aws-lc-sys v0.47.0` and `provider::a_client_hello_comes_out_of_aws_lc ... ok`.
   - Measured on 4 cores: build phase 2 min 51 s, checkPhase compile 4 min 09 s, then the workspace tests.
   - Run it in the background with a 40 min limit, and wait for it to finish.

**Gate G1: CI.** Required before any code that uses `eml_tls` (R1).
1. Commit T1 and push to the session's designated feature branch.
2. Trigger the workflow with the GitHub MCP tool `actions_run_trigger`: `workflow_dispatch` of `ci.yml`, with ref = that branch.
3. Find the run with `actions_list` (runs of `ci.yml` on the branch). Poll `actions_get` until it completes.
4. Pass condition: every job concludes `success`. That is `test` on the 4 OSes, `fmt`, `verus`, `miri` ×2 and `asan` ×3.
5. On failure, read `get_job_logs` for the failed jobs and apply the decision rule below.
6. Also record each `test` job's duration from `actions_get`. If any is above 45 min (the limit is 60), report it to the user before T2.
7. If dispatch is refused (permissions or tool error), stop. Ask the user to trigger CI on the branch (Actions → ci → Run workflow, or a PR) and report the result. Do not open a PR without the user's request.

**Gate G2: nix build on aarch64-darwin.** Required before T2, in parallel with G1. Ask the user to run `nix build -L` on their aarch64-darwin machine at the T1 commit and report the exit status and the `provider::a_client_hello_comes_out_of_aws_lc` line. A failure there is handled like a nix build failure in the rule below.

**Decision rule when T1 fails.** Classify each failed job by its log.

| Class | Log signature | Action |
|---|---|---|
| a. Build script | `failed to run custom build command for aws-lc-sys` (any cause: missing tool, compiler error, NASM or prebuilt problem) on any CI OS, or on `nix build` (x86_64-linux or the user's aarch64-darwin) | Switch to ring (below). A failure on a single OS is enough. |
| b. Link | Linker error naming `aws_lc_*` symbols or `libaws_lc*` | Same as a. |
| c. Runtime | The build passes, but `a_client_hello_comes_out_of_aws_lc` fails or crashes | Same as a. |
| d. Timeout or infrastructure | Job cancelled at `timeout-minutes`, network or fetch error, runner lost | Re-run the failed jobs once (`actions_run_trigger` with the re-run method, or dispatch again). If it repeats, stop and report to the user with the job durations. |
| e. Unrelated | A failure outside `eml_tls`, aws-lc or the workflow | Not caused by T1, which changes no other code. Stop and report to the user; do not switch providers. |

**Switching to ring**
1. Change the workspace dependency to `rustls = { version = "0.23.46", default-features = false, features = ["ring", "std", "tls12"] }`.
2. In the test, use `rustls::crypto::ring::default_provider()` and rename it to `a_client_hello_comes_out_of_ring`.
3. In the same commit, rewrite the provider statements in docs/future/async.md 「TLS と HTTP」, architecture.md and the workspace comment.
4. Redo the local completion steps, G1 and G2.
5. If ring fails in class a, b or c on any OS, stop the stage and report to the user. The design must be decided again. Do not add tools or env vars.

### T2: the `tls` features and the CI guards (local verification)

Its position is constrained by R2 and R4.

**Files**
- **`crates/eml_io/Cargo.toml`:** add `[features]` with `tls = ["dep:eml_tls"]`. Comment points: only the test driver's terminator and the name `tls.localhost` use it; the real driver does not. Add `eml_tls = { workspace = true, optional = true }`.
- **`crates/eml_vm/Cargo.toml`:** add `[features]` with `tls = ["dep:eml_tls", "eml_io/tls"]`. Add `eml_tls = { workspace = true, optional = true }`.
- **`crates/eml_cli/Cargo.toml`:**
  - Set `default = ["run", "tls"]`.
  - Add `tls = ["run", "eml_vm/tls"]`, and extend the feature comment: with tls off, the `Std.Tls` arms throw NotAvailable.
  - The dev-dependency `eml_test_support = { workspace = true, features = ["run"] }` stays as it is.
- **`.github/workflows/ci.yml`, `test` job:**
  - In the step "cargo clippy of the non-default features", after `eml_cli`'s `--features core` line, add:

    ```
              cargo clippy -p eml_cli --no-default-features --features run -- -D warnings
              cargo clippy -p eml_io -p eml_vm --all-targets -- -D warnings
    ```
  - After that step, add:

    ```yaml
          # ワークスペースの実行で eml_io と eml_vm の tls が有効になり、eml_vm だけを選ぶ Miri と ASan には eml_tls が入らず、
          # .#cross の clippy が eml_tls なしで組めることを、feature の解決で確かめる (docs/implementation/testing.md の「feature の組み合わせの確認」)
          - name: feature resolution of tls
            if: ${{ !cancelled() && steps.test.outcome != 'skipped' }}
            run: |
              test "$(cargo tree --workspace -e features -i eml_io | grep -c 'eml_io feature "tls"')" -ge 1
              test "$(cargo tree --workspace -e features -i eml_vm | grep -c 'eml_vm feature "tls"')" -ge 1
              test "$(cargo tree -p eml_vm -p eml_io -e normal,dev,build --target all | grep -c -E 'eml_tls|aws-lc|ring v')" -eq 0
              test "$(cargo tree --workspace --exclude eml_tls --no-default-features --features eml_cli/run,eml_test_support/run -e normal,dev,build --target all | grep -c -E 'eml_tls|aws-lc|ring v')" -eq 0
    ```

    `grep -c` exits 1 on a zero count, but this does not abort the step. In bash, a failing command substitution inside `test` does not trigger errexit, and the `test` result decides the step.
- **`docs/implementation/testing.md`:**
  - 「feature の組み合わせの確認」:
    - There are 9 clippy combinations: the 7 existing ones, plus `eml_cli --no-default-features --features run` (lib, tls off), plus `-p eml_io -p eml_vm --all-targets` (tls off, with tests).
    - Unification: in the workspace, `eml_cli`'s default enables `tls` in eml_vm and eml_io. Selecting `-p eml_io` or `-p eml_vm` turns it off.
    - Running those crates' tls-on tests alone: `cargo nextest run -p eml_io -p eml_vm --features eml_io/tls,eml_vm/tls`.
    - The four `cargo tree` assertions and what each one protects.
    - `cargo test -p eml_cli --no-default-features --features run` is unsupported, because the UI tests assume `tls`.
  - 「CI」, the `test` bullet: the 9 combinations and the feature-resolution step.
  - 「よく使うコマンド」 line 661: change the comment to `(なし、types、core、run (tls なし)。eml_test_support は hir も。eml_io と eml_vm は --all-targets で tls なし)`.
- **`CLAUDE.md`:**
  - The comment on line 35: `# non-default features (none, types, core, run without tls; eml_test_support also hir; eml_io + eml_vm --all-targets without tls)`.
  - The `eml_cli` line: `stage features types < core < run; tls (default) adds Std.Tls`.
  - The `eml_tls` line: add `; eml_vm and eml_io reach it through their feature tls`.
- **`docs/implementation/architecture.md`:**
  - The `eml_cli` bullet: the default is `run` and `tls`, and the chain is `tls` → `eml_vm/tls` → `eml_io/tls`.
  - The `eml_tls` bullet: it is a dependency only under `tls`.
  - The dependency order is `eml_cli` > `eml_vm` > `eml_io` > `eml_tls`, alongside `eml_io` > `eml_rt`.

**Test-change kind:** none.

**Completion**
1. All four `cargo tree` lines of the new step, run locally in bash, exit 0.
2. Both new clippy lines exit 0.
3. `cargo nextest run` passes everything.
4. The `.#cross` command exits 0 for all 3 targets.
5. `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` exits 0.

### T3: the tls-off test step (in the commit of the first `tls_off` module, R3)

**Files**
- **`.github/workflows/ci.yml`**, right after the step `cargo nextest run` (id `test`):

  ```yaml
        # ワークスペースの実行では eml_cli の既定が tls を有効にするので、tls を外した処理系の振る舞いは
        # eml_io と eml_vm だけを選んで流す (docs/implementation/testing.md の「feature の組み合わせの確認」)
        - name: cargo nextest run without tls
          if: ${{ !cancelled() && steps.test.outcome != 'skipped' }}
          run: cargo nextest run -p eml_io -p eml_vm --failure-output immediate-final -E 'test(tls_off::)'
  ```
- **`docs/implementation/testing.md`:**
  - 「feature の組み合わせの確認」: the step and its command, and the rule that the `tls_off` modules are not OS-gated. A filter that matches nothing fails with exit 4, so the step also catches a renamed module.
  - 「CI」: the `test` bullet.
  - 「よく使うコマンド」: add the command.
- **`CLAUDE.md` Commands:** add `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'   # the tls-off behaviour (a workspace run has tls on through eml_cli's default)`.

**Completion**
1. `cargo nextest run -p eml_io -p eml_vm -E 'test(tls_off::)'` reports N tests passed, and at least one of them comes from each crate that has the module.
2. The workspace `cargo nextest run` passes, with the `tls_off` modules compiled out.
3. actionlint exits 0.
4. The branch's next CI run is green. No dedicated gate is needed.

### Stage-close items

- Tell the user: the `eml` binary now includes aws-lc (ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause …), rustls (Apache-2.0 OR ISC OR MIT) and webpki-roots data (CDLA-Permissive-2.0).
  - `flake.nix` `meta.license` stays `mit`, because it states eml's own license.
  - Whether to list the bundled licenses is the user's decision.
- The tables in benchmarks.md are refreshed under the usual rule.
- S1 adds the reading-map row in docs/README.md.

## 6. Unresolved items with defaults

- **NASM on windows-2025.** Unknown. It does not matter, because both paths work; the G1 log shows which one was taken.
- **`nix build` on aarch64-linux.** Not gated (A5). If the user has such a machine, it may be run alongside G2.

## 7. Additional checks performed

- **The nixpkgs `cargoCheckHook` source** runs `cargo test` with `cargoTestFlags` (unset in this flake) and `checkFlags`. In a virtual workspace that means every member [V].
- **The asan and miri jobs do not reach aws-lc** [V by `cargo tree`]:
  - asan: `-p eml_io`, `-p eml_vm`;
  - miri: `-p eml_rt`, `-p eml_vm`, `-p eml_io`.

  verus and fmt are unaffected.
- **The `cargo tree` guard** detects a forbidden dev-dependency: the count went from 0 to 4 after one was added to eml_io [V].
- **Housekeeping.** `/home/user/eml` was not modified, and no background task is running.
