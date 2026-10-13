# S9a `eml_rt` の核 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** VM とネイティブで共有するランタイムの核 `crates/eml_rt` を作る。語、ヘッダ、ページの確保器、局所と共有の RC、`share`、`lin` ビット、reuse の部品、型の引数で選ぶ検査モードを持ち、Miri と Verus で確かめる。

**Architecture:** 下から積む。語とヘッダの型 (Task 2)、RC の遷移の純粋な関数 (Task 3)、ページの確保器 (Task 4) を作り、その上に `Heap<M: Mode>` の確保、RC、解放の連鎖、検査モードを置く (Task 5)。共有の経路と `release` (Task 6)、reuse の token (Task 7) を足し、Verus (Task 8、Task 9) と Miri (Task 10) で確かめ、計測して (Task 11)、文書に移す (Task 12)。`eml_rt` はどの crate からも使わないので、既存のテストは変わらない。

**Tech Stack:** Rust (edition 2024)、Miri (nightly、rust-overlay の devShell)、Verus 0.2026.10.04.426d8b0 (Nix の外。rustup の Rust 1.98.1)、macOS の `/usr/bin/time -l`。

**Spec:** `docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md`。実装する人は、各タスクの前に spec の該当する節を読む。spec が規範で、計画とずれたら spec に従い、ずれを報告する。

| 見出し | 中身 |
|---|---|
| Task 1 | Miri の devShell と、Kani が動くかの試し (Kani は後に Verus に置き換えた) |
| Task 2 | crate、`sync` (Task 10 で消す)、`Mode`、`Word`、`Meta` と `Kind` とヘッダ |
| Task 3 | RC の遷移 (`rc.rs`) |
| Task 4 | ページの確保器 (`alloc.rs`) |
| Task 5 | `Heap`: 確保、RC、解放の連鎖、フィールド、`Foreign`、検査モード、生存の列挙、リーク |
| Task 6 | `share`、共有の側の RC、`is_last` と `release`、不変条件の検査 |
| Task 7 | reuse の token |
| Task 8 | Verus の土台と `share` の証明、写しの照合のテスト |
| Task 9 | Verus の証明 (ヘッダ、即値、RC の遷移、判定) |
| Task 10 | Miri と、差し替えの層 (`sync`) を消すこと |
| Task 11 | 計測 (`examples/rt_bench.rs`、`bench/rt.sh`) |
| Task 12 | 文書、spec と計画の削除 |

## Global Constraints

- 各タスクの終わりに次が通る。途中のタスクでもワークスペースを壊したままコミットしない
  - `cargo test` (ワークスペース全体。`eml_rt` は `crates/*` なので自動で入る)
  - `cargo fmt --check` が差分を出さない
  - `cargo clippy --all-targets` が `eml_rt` の外に警告を出さない。`eml_rt` では、Task 2 から Task 4 の間だけ、まだ使わない `pub(crate)` の項目への `dead_code` の警告を許す。Task 5 の終わりからは `cargo clippy -p eml_rt --all-targets -- -D warnings` が通る
  - `cargo test -p eml_cli --test integration citations`
- 既存のテストの期待値と成否は、どのタスクでも変えない (spec の「テストの変更」は「なし」)
- `eml_rt` は依存を持たない。Verus の写し (`crates/eml_rt/verus/`) は Cargo のビルドに入れない
- Verus は Nix の外で `crates/eml_rt/verus/check.sh` で流す (導入は Task 8)。Task 8 からは、各タスクの終わりに `check.sh` も通る
- `unsafe` は spec の「`unsafe` の核」の核に閉じる。`&Header` と `&mut Header` は作らない。`rc` は `&AtomicI32` で、`meta` は生のポインタで読み書きする。整数からポインタを作らない (`without_provenance_mut` と `map_addr` だけを使う)
- edition 2024 なので、`unsafe fn` の中でも `unsafe` の操作は `unsafe { }` で囲む
- 名前 (各タスクで共通に使う)
  - `Word` (Task 2): `Word::imm(i64)`、`Word::int(i64) -> Option<Word>`、`Word::UNIT`、`Word::EMPTY`、`is_imm`、`is_ptr`、`is_empty`、`imm_value`、`bits`、`Word::MIN_INT` / `MAX_INT`、crate の中の `from_header(NonNull<Header>)` と `header() -> NonNull<Header>`
  - `Kind { Data, BigInt, Foreign, Free }`、`Meta::new(tag: u16, scan: u8, kind: Kind, lin: bool)`、`Meta::{tag, scan, kind, lin, with_lin, same_shape}`、`Meta::EXTENDED_SCAN` (Task 2)
  - `meta::{Header, HEADER_WORDS, init, rc, meta, set_meta, slot}` (Task 2)
  - `rc::{LOCAL_MAX, DUP_LIMIT, DROP_LIMIT, DupStep, DecrefStep, dup_step, decref_step, mark_step, dup_shared, decref_shared, is_last}` (Task 3)
  - `alloc::{Pages, PAGE_BYTES}`、`Pages::{new, alloc, free, bytes, block_words, block_bytes, fits, for_each_block}` (Task 4)
  - `Heap<M>` と `Stats` (Task 5)、`Token` (Task 7)、`HeapError` (Task 5)
  - 検査モードの前置き `heap::check::{Prefix, State, PREFIX_WORDS, UNCOUNTED, prefix}` (Task 5)
- `Heap` の公開の操作の名前と引数は spec の「核の操作」の表のとおりにする。`alloc` と `con_at` はペイロードの語の数を受け取る
- 日本語のコメントと文書は、書く前に `yomiyasu:yomiyasu` のスキルを呼び、その規則に従う。コメントは「なぜ」を書く。規則を指すときは、module の先頭のコメントでは `docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「見出し」` の形で、ほかのコメントでは `spec の「見出し」` の形で引く。引いてよい見出しは次のものだけである。Task 12 で `docs/spec/object-model.md` に同じ見出しで移し、どちらの形もそのパスに置き換えるためである: 語、ダミーの値、ヘッダ、ペイロード、種類、ページ、確保と解放、生存の列挙、符号、局所の速い道、上限、共有の側、遷移の関数、`share`、`lin` ビット、核の操作、reuse、モードの選び方、前置き、検査モードの token、不死の物体の数え方、報告する誤り、論理的な worker ID、`unsafe` の核、受け入れる制約。検証の節の「Verus」も引いてよい。ただし「Verus」は module の先頭でも `spec の「Verus」` の形で引く。Task 12 でこれだけを `docs/implementation/testing.md` の同じ見出しに移すためである
- コメントに計画のタスクの番号を書かない
- コミットメッセージは英語で、既存の形 (命令形の1行目、本文に理由) に合わせる。各コミットの末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01YLHZowtbHVXEbsTMFYwSMN
  ```

- `git diff` は `--no-ext-diff` で見る
- テストの大きさは Miri で遅くなりすぎないように、`cfg!(miri)` で小さくする (`const LONG: usize = if cfg!(miri) { 300 } else { 100_000 };`)

## Review Focus

spec から出てくるが、spec の文が直接は名指さない入力のうち、使う人が最初に踏みそうなもの。各行の確かめは、対応するタスクに入れてある。

- 速いモードでちょうど 128 語、検査モードでは前置きで大きい物体の道に移る大きさ。どちらのモードでも確保、読み書き、解放、列挙が正しい (Task 5 の `the_boundary_between_small_and_large_works_in_both_modes`)
- 大きさの級をまたいで確保と解放を混ぜる列。空きリストが級の間で混ざらず、物体の中身が壊れない (Task 5 の `mixed_sizes_keep_the_free_lists_apart`)
- 大きさ 0 の Rust の値を持つ `Foreign`。確保、型の照らし合わせ、解放が正しい (Task 5 の `a_zero_sized_foreign_value_fits`)
- 長いリストの `share`。たどりが再帰せず、その後の共有の側の解放も再帰しない (Task 6 の `a_deep_list_is_shared_without_recursion`)
- 使っていない token、不死の物体、大きい物体、`Foreign` が残ったまま `Heap` を落とす形。破棄処理が1回だけ呼ばれ、Miri が漏れを報告しない (Task 7 の `a_heap_dropped_with_an_outstanding_token_and_large_objects_is_clean`)

---

### Task 1: Miri の devShell と Kani の試し

**Files:**
- Modify: `flake.nix`
- Modify: `flake.lock` (自動で更新される)

**Interfaces:**
- Consumes: なし
- Produces: `nix develop .#miri` (nightly、`miri`、`rust-src`)。Kani を導入する方法の結論 (Kani は Verus に置き換えたので、後のタスクは使わない)

- [ ] **Step 1: flake に rust-overlay と Miri の devShell を足す**

`flake.nix` の `inputs` と `perSystem` を次のようにする。ふだんの devShell と `packages` は変えない。

```nix
{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-darwin"
      ];

      perSystem =
        { pkgs, system, ... }:
        let
          # Miri だけが nightly を要る。ふだんの devShell と nix build は nixpkgs の stable のままにする
          # (docs/implementation/testing.md)
          overlaid = import inputs.nixpkgs {
            inherit system;
            overlays = [ inputs.rust-overlay.overlays.default ];
          };
          nightly = overlaid.rust-bin.selectLatestNightlyWith (
            toolchain:
            toolchain.minimal.override {
              extensions = [
                "miri"
                "rust-src"
              ];
            }
          );
        in
        {
          packages = rec {
            # (今のまま)
          };

          devShells = {
            default = pkgs.mkShell {
              # (今のまま)
            };
            miri = pkgs.mkShell {
              packages = [ nightly ];
            };
          };
        };
    };
}
```

`packages` と `devShells.default` の中身は今のファイルから写す。`(今のまま)` の行は残さない。

- [ ] **Step 2: devShell で Miri が動くことを確かめる**

Run: `nix develop .#miri -c cargo miri --version && nix develop .#miri -c cargo miri setup`
Expected: `miri 0.1.0 (...)` の版の行が出て、`setup` が sysroot を作って終わる。`flake.lock` に `rust-overlay` が足される

- [ ] **Step 3: Kani を試す**

scratchpad に小さな crate を作り、1つのハーネスを流す。

```sh
cargo install --locked kani-verifier
cargo kani setup
probe=/private/tmp/claude-501/-Users-arakaki-Projects-eml/310bf5be-9f75-4b63-a0f2-129c444f2915/scratchpad/kani-probe
cargo new --lib "$probe"
cat > "$probe/src/lib.rs" <<'EOF'
#[cfg(kani)]
#[kani::proof]
fn wrapping_add_round_trips() {
    let a: i32 = kani::any();
    let b: i32 = kani::any();
    assert_eq!(a.wrapping_add(b).wrapping_sub(b), a);
}
EOF
(cd "$probe" && cargo kani)
cargo kani --version
```

Expected: `VERIFICATION:- SUCCESSFUL` が出る。`cargo kani --version` が版を出す。Kani の toolchain の rustc が 1.85 以上 (edition 2024 と、`NonNull::map_addr`、`ptr::without_provenance_mut`、const の `div_ceil` を使えること) であることを、`~/.kani/` の toolchain の `rustc -V` で確かめる。

- [ ] **Step 4: 試しの結果で止まるか決める**

Kani が動かない (setup が失敗する、ハーネスが走らない、rustc が 1.85 より古い) なら、ここで止め、出力を添えて報告する。代わりの確かめ方を相談で決めるまで Task 2 に進まない (spec の「目的」の完了の条件)。動いたら、導入の方法 (`cargo install --locked kani-verifier` と `cargo kani setup`、版、Nix の devShell の外で動かしたか) を書き留める。Task 12 で `docs/implementation/testing.md` に書く。

- [ ] **Step 5: コミット**

```bash
git add flake.nix flake.lock
git commit -m "Add a nightly devShell for Miri

eml_rt's unsafe core is checked with Miri, which needs a nightly toolchain
with the miri and rust-src components. The default devShell and nix build
stay on the nixpkgs stable toolchain.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YLHZowtbHVXEbsTMFYwSMN"
```

---

### Task 2: crate、語、ヘッダ

**Files:**
- Create: `crates/eml_rt/Cargo.toml`
- Create: `crates/eml_rt/src/lib.rs`
- Create: `crates/eml_rt/src/sync.rs`
- Create: `crates/eml_rt/src/mode.rs`
- Create: `crates/eml_rt/src/word.rs`
- Create: `crates/eml_rt/src/meta.rs`

**Interfaces:**
- Consumes: なし
- Produces: `Word`、`Kind`、`Meta`、`Mode` / `Fast` / `Checked`、`meta::{Header, HEADER_WORDS, init, rc, meta, set_meta, slot}`、`sync::AtomicI32`

- [ ] **Step 1: crate の骨組みを作る**

`crates/eml_rt/Cargo.toml`:

```toml
[package]
name = "eml_rt"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
doctest = false

[dependencies]

[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ["cfg(loom)", "cfg(kani)"] }
```

`crates/eml_rt/src/sync.rs`:

```rust
//! atomic とページの確保の層 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「`unsafe` と同期の層」)。
//! 後で `cfg(loom)` のときに loom の型へ差し替えるので、核はこの module から atomic を使う。

pub(crate) use std::sync::atomic::AtomicI32;
```

`crates/eml_rt/src/mode.rs`:

```rust
//! 速いモードと検査モード (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「モードの選び方」)。
//! 型の引数で選ぶので、速いモードでは検査の分岐が単相化で消える。

mod sealed {
    pub trait Sealed {}
}

pub trait Mode: sealed::Sealed + 'static {
    const CHECKED: bool;
}

#[derive(Debug)]
pub enum Fast {}

#[derive(Debug)]
pub enum Checked {}

impl sealed::Sealed for Fast {}
impl sealed::Sealed for Checked {}

impl Mode for Fast {
    const CHECKED: bool = false;
}

impl Mode for Checked {
    const CHECKED: bool = true;
}
```

`crates/eml_rt/src/lib.rs`:

```rust
//! VM とネイティブで共有するランタイムの核 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md)。語、ヘッダ、
//! 確保器、RC、`share`、reuse の部品、検査モードを持つ。まだどの crate からも使わない。

mod meta;
mod mode;
mod sync;
mod word;

pub use meta::{Kind, Meta};
pub use mode::{Checked, Fast, Mode};
pub use word::Word;

#[cfg(test)]
mod tests {
    use super::*;

    fn send_and_sync<T: Send + Sync>() {}

    #[test]
    fn words_cross_threads() {
        send_and_sync::<Word>();
    }
}
```

- [ ] **Step 2: 語のテストを書く**

`crates/eml_rt/src/word.rs` の末尾に置くテスト (本体は Step 4 で書く):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediates_round_trip_at_the_63_bit_bounds() {
        for n in [0, 1, -1, Word::MIN_INT, Word::MAX_INT] {
            let w = Word::int(n).unwrap();
            assert!(w.is_imm());
            assert!(!w.is_ptr());
            assert_eq!(w.imm_value(), n);
        }
        assert_eq!(Word::int(Word::MAX_INT + 1), None);
        assert_eq!(Word::int(Word::MIN_INT - 1), None);
        assert_eq!(Word::int(i64::MIN), None);
    }

    #[test]
    fn unit_is_the_immediate_zero_whose_bits_are_one() {
        assert_eq!(Word::UNIT, Word::imm(0));
        assert_eq!(Word::UNIT.bits(), 1);
    }

    #[test]
    fn the_empty_word_is_neither_an_immediate_nor_a_pointer() {
        assert!(Word::EMPTY.is_empty());
        assert!(!Word::EMPTY.is_imm());
        assert!(!Word::EMPTY.is_ptr());
    }
}
```

- [ ] **Step 3: ヘッダの欄のテストを書く**

`crates/eml_rt/src/meta.rs` の末尾に置くテスト:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_keeps_each_field() {
        let m = Meta::new(0xBEEF, 7, Kind::Foreign, true);
        assert_eq!(m.tag(), 0xBEEF);
        assert_eq!(m.scan(), 7);
        assert_eq!(m.kind(), Kind::Foreign);
        assert!(m.lin());
        let plain = m.with_lin(false);
        assert!(!plain.lin());
        assert_eq!((plain.tag(), plain.scan(), plain.kind()), (0xBEEF, 7, Kind::Foreign));
    }

    #[test]
    fn same_shape_ignores_only_the_lin_bit() {
        let m = Meta::new(3, 2, Kind::Data, false);
        assert!(m.same_shape(m.with_lin(true)));
        assert!(!m.same_shape(Meta::new(4, 2, Kind::Data, false)));
        assert!(!m.same_shape(Meta::new(3, 1, Kind::Data, false)));
        assert!(!m.same_shape(Meta::new(3, 2, Kind::BigInt, false)));
    }

    #[test]
    fn only_data_becomes_a_token_and_only_foreign_has_a_finalizer() {
        for kind in [Kind::Data, Kind::BigInt, Kind::Foreign, Kind::Free] {
            assert_eq!(kind.tokenable(), kind == Kind::Data);
            assert_eq!(kind.has_finalizer(), kind == Kind::Foreign);
            assert_eq!(kind.scan_start(), 0);
        }
    }

    #[test]
    fn the_header_is_one_word() {
        assert_eq!(HEADER_WORDS, 1);
    }
}
```

- [ ] **Step 4: 語を書く**

`crates/eml_rt/src/word.rs` の本体 (テストの前に置く):

```rust
//! 8 バイトの語 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「語」)。下位ビットが 1 なら即値、0 なら
//! ヘッダを指すポインタである。`u64` でなくポインタ型で持つのは、strict provenance の下でポインタの出所を失わないためである。

use std::fmt;
use std::ptr::{self, NonNull};

use crate::meta::Header;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Word(*mut ());

// 語は値で、スレッドをまたいでよいかは型でなく `share` の規約が守る
unsafe impl Send for Word {}
unsafe impl Sync for Word {}

impl Word {
    pub const MIN_INT: i64 = -(1 << 62);
    pub const MAX_INT: i64 = (1 << 62) - 1;
    /// `()`。値 0 の即値で、語の値は 1 である。生の `unit` の値もこの語の値にする。
    pub const UNIT: Word = Word::imm(0);
    /// 空の token と、まだ値を入れていない位置。即値でもポインタでもない。
    pub const EMPTY: Word = Word(ptr::null_mut());

    /// 即値 `(n << 1) | 1`。n は 63 ビットに収まらなければならない。
    pub const fn imm(n: i64) -> Word {
        assert!(Word::MIN_INT <= n && n <= Word::MAX_INT);
        Word(ptr::without_provenance_mut(((n << 1) | 1) as usize))
    }

    /// 63 ビットに収まる `Int` を即値にする。収まらなければ `None` で、呼ぶ側が `BigInt` の物体にする。
    pub fn int(n: i64) -> Option<Word> {
        (Word::MIN_INT..=Word::MAX_INT).contains(&n).then(|| Word::imm(n))
    }

    pub fn is_imm(self) -> bool {
        self.0.addr() & 1 == 1
    }

    pub fn is_ptr(self) -> bool {
        self.0.addr() & 1 == 0 && !self.0.is_null()
    }

    pub fn is_empty(self) -> bool {
        self.0.is_null()
    }

    /// 即値の値。算術右シフトで1ビットずらす。
    pub fn imm_value(self) -> i64 {
        debug_assert!(self.is_imm());
        (self.0.addr() as i64) >> 1
    }

    /// 語のビット列。VM が生のスロットに置くときに使う。
    pub fn bits(self) -> usize {
        self.0.addr()
    }

    pub(crate) fn from_header(h: NonNull<Header>) -> Word {
        Word(h.as_ptr().cast())
    }

    pub(crate) fn header(self) -> NonNull<Header> {
        debug_assert!(self.is_ptr());
        // ポインタの語は確保したヘッダから作るので、null にならない
        unsafe { NonNull::new_unchecked(self.0.cast()) }
    }
}

impl fmt::Debug for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_imm() {
            write!(f, "Word::imm({})", self.imm_value())
        } else if self.is_empty() {
            f.write_str("Word::EMPTY")
        } else {
            write!(f, "Word({:#x})", self.0.addr())
        }
    }
}
```

- [ ] **Step 5: ヘッダと種類を書く**

`crates/eml_rt/src/meta.rs` の本体:

```rust
//! 物体のヘッダと種類 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「ヘッダ」と「種類」)。

use std::fmt;
use std::ptr::NonNull;

use crate::sync::AtomicI32;
use crate::word::Word;

/// 物体の種類。`info` の下位4ビットに入る。種類ごとの欄 (名前、走査の始まり、token、破棄処理) は、型ごとの表を
/// 持たずにここの `match` で決める。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Data = 0,
    BigInt = 1,
    Foreign = 2,
    /// 確保器が空いたブロックに付ける。生きている物体はこの種類を持たない。
    Free = 15,
}

impl Kind {
    fn from_bits(bits: u32) -> Kind {
        match bits {
            0 => Kind::Data,
            1 => Kind::BigInt,
            2 => Kind::Foreign,
            15 => Kind::Free,
            _ => unreachable!("an unknown object kind {bits}"),
        }
    }

    /// リークの報告の種類の名前。
    pub fn name(self) -> &'static str {
        match self {
            Kind::Data => "Data",
            Kind::BigInt => "BigInt",
            Kind::Foreign => "Foreign",
            Kind::Free => "Free",
        }
    }

    /// 走査する語 (`scan` が 255 なら数の語) がペイロードの何語目から始まるか。種類を足すときに決め忘れないように、
    /// 種類ごとに書く。
    pub(crate) fn scan_start(self) -> usize {
        match self {
            Kind::Data | Kind::BigInt | Kind::Foreign | Kind::Free => 0,
        }
    }

    pub(crate) fn tokenable(self) -> bool {
        matches!(self, Kind::Data)
    }

    pub(crate) fn has_finalizer(self) -> bool {
        matches!(self, Kind::Foreign)
    }
}

/// ヘッダの `meta`。tag (16 ビット) | scan (8 ビット) | info (8 ビット。最上位が `lin` ビット、下位4ビットが種類)。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Meta(u32);

impl Meta {
    /// `scan` のこの値は、走査する語の数がペイロードの生の1語にあることを表す。
    pub const EXTENDED_SCAN: u8 = 255;
    const LIN: u32 = 1 << 31;
    const KIND_SHIFT: u32 = 24;
    const KIND_MASK: u32 = 0xF;

    pub const fn new(tag: u16, scan: u8, kind: Kind, lin: bool) -> Meta {
        let lin = if lin { Meta::LIN } else { 0 };
        Meta(tag as u32 | ((scan as u32) << 16) | ((kind as u32) << Meta::KIND_SHIFT) | lin)
    }

    pub const fn tag(self) -> u16 {
        self.0 as u16
    }

    pub const fn scan(self) -> u8 {
        (self.0 >> 16) as u8
    }

    pub fn kind(self) -> Kind {
        Kind::from_bits((self.0 >> Meta::KIND_SHIFT) & Meta::KIND_MASK)
    }

    pub const fn lin(self) -> bool {
        self.0 & Meta::LIN != 0
    }

    pub const fn with_lin(self, lin: bool) -> Meta {
        Meta((self.0 & !Meta::LIN) | if lin { Meta::LIN } else { 0 })
    }

    /// `lin` ビットを除いて同じか。
    pub const fn same_shape(self, other: Meta) -> bool {
        (self.0 ^ other.0) & !Meta::LIN == 0
    }
}

impl fmt::Debug for Meta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Meta")
            .field("tag", &self.tag())
            .field("scan", &self.scan())
            .field("kind", &self.kind())
            .field("lin", &self.lin())
            .finish()
    }
}

/// 物体のヘッダ。ポインタはここを指す。`&Header` は作らず、`rc` は共有の参照で、`meta` は生のポインタで読み書きする。
/// `meta` を書くのは、ほかのスレッドがその物体を読めないときだけである。
#[repr(C)]
pub(crate) struct Header {
    rc: AtomicI32,
    meta: u32,
}

/// ヘッダの語の数。loom の型に差し替えたビルドではヘッダが大きくなるので、8 と書かずに大きさから出す。
pub(crate) const HEADER_WORDS: usize = size_of::<Header>().div_ceil(8);

/// # Safety
/// `h` は確保したブロックのヘッダの位置を指す。
pub(crate) unsafe fn init(h: NonNull<Header>, rc: i32, meta: Meta) {
    unsafe {
        h.as_ptr().write(Header {
            rc: AtomicI32::new(rc),
            meta: meta.0,
        })
    }
}

/// # Safety
/// `h` は初期化したヘッダを指す。
pub(crate) unsafe fn rc<'a>(h: NonNull<Header>) -> &'a AtomicI32 {
    unsafe { &(*h.as_ptr()).rc }
}

/// # Safety
/// `h` は初期化したヘッダを指す。
pub(crate) unsafe fn meta(h: NonNull<Header>) -> Meta {
    unsafe { Meta((&raw const (*h.as_ptr()).meta).read()) }
}

/// # Safety
/// `h` は初期化したヘッダを指し、ほかのスレッドはその物体を読めない。
pub(crate) unsafe fn set_meta(h: NonNull<Header>, meta: Meta) {
    unsafe { (&raw mut (*h.as_ptr()).meta).write(meta.0) }
}

/// ペイロードの物理的な i 語目の位置。
///
/// # Safety
/// `h` はペイロードが i + 1 語以上あるブロックのヘッダを指す。
pub(crate) unsafe fn slot(h: NonNull<Header>, i: usize) -> *mut Word {
    unsafe { h.as_ptr().cast::<Word>().add(HEADER_WORDS + i) }
}
```

- [ ] **Step 6: テストを流す**

Run: `cargo test -p eml_rt`
Expected: `word::tests` の3つ、`meta::tests` の4つ、`tests::words_cross_threads` が通る。`dead_code` の警告は出てよい (Global Constraints)

- [ ] **Step 7: fmt と clippy**

Run: `cargo fmt && cargo clippy -p eml_rt --all-targets`
Expected: `dead_code` のほかの警告がない

- [ ] **Step 8: コミット**

```bash
git add crates/eml_rt Cargo.lock
git commit -m "Add the eml_rt crate with words and headers

Words are pointer-typed so immediates keep strict provenance; headers are
rc plus a packed meta (tag, scan, lin bit, kind) so allocation and reuse
write the header with one store.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YLHZowtbHVXEbsTMFYwSMN"
```

---

### Task 3: RC の遷移

**Files:**
- Create: `crates/eml_rt/src/rc.rs`
- Modify: `crates/eml_rt/src/lib.rs` (`mod rc;`)
- Modify: `crates/eml_rt/src/sync.rs` (`Ordering`)

**Interfaces:**
- Consumes: `sync::AtomicI32`
- Produces: `rc::{LOCAL_MAX, DUP_LIMIT, DROP_LIMIT}`、`DupStep { Store(i32), Shared, Skip }`、`DecrefStep { Store(i32), Free, Shared, Skip }`、`dup_step(i32) -> DupStep`、`decref_step(i32) -> DecrefStep`、`mark_step(i32) -> Option<i32>`、`dup_shared(&AtomicI32)`、`decref_shared(&AtomicI32) -> bool`、`is_last(&AtomicI32) -> bool`

- [ ] **Step 1: テストを書く**

`crates/eml_rt/src/rc.rs` の末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dup_takes_the_fast_path_only_below_the_local_cap() {
        assert_eq!(dup_step(1), DupStep::Store(2));
        assert_eq!(dup_step(LOCAL_MAX - 1), DupStep::Store(LOCAL_MAX));
        assert_eq!(dup_step(LOCAL_MAX), DupStep::Skip);
        assert_eq!(dup_step(0), DupStep::Skip);
        assert_eq!(dup_step(-1), DupStep::Shared);
        assert_eq!(dup_step(DUP_LIMIT), DupStep::Shared);
        assert_eq!(dup_step(DUP_LIMIT - 1), DupStep::Skip);
        assert_eq!(dup_step(i32::MIN), DupStep::Skip);
    }

    #[test]
    fn decref_frees_at_one_and_skips_stuck_counts() {
        assert_eq!(decref_step(2), DecrefStep::Store(1));
        assert_eq!(decref_step(1), DecrefStep::Free);
        assert_eq!(decref_step(LOCAL_MAX - 1), DecrefStep::Store(LOCAL_MAX - 2));
        assert_eq!(decref_step(LOCAL_MAX), DecrefStep::Skip);
        assert_eq!(decref_step(0), DecrefStep::Skip);
        assert_eq!(decref_step(-1), DecrefStep::Shared);
        assert_eq!(decref_step(DROP_LIMIT), DecrefStep::Shared);
        assert_eq!(decref_step(DROP_LIMIT - 1), DecrefStep::Skip);
    }

    #[test]
    fn marking_flips_local_counts_and_leaves_the_rest() {
        assert_eq!(mark_step(3), Some(-3));
        assert_eq!(mark_step(LOCAL_MAX), Some(i32::MIN));
        assert_eq!(mark_step(-DROP_LIMIT), Some(DROP_LIMIT));
        assert_eq!(mark_step(-DROP_LIMIT + 1), Some(i32::MIN));
        assert_eq!(mark_step(0), None);
        assert_eq!(mark_step(-2), None);
    }

    #[test]
    fn shared_counts_move_atomically_and_find_the_last_reference() {
        let rc = AtomicI32::new(-2);
        dup_shared(&rc);
        assert_eq!(rc.load(Ordering::Relaxed), -3);
        assert!(!decref_shared(&rc));
        assert!(!decref_shared(&rc));
        assert!(decref_shared(&rc));
        assert_eq!(rc.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn is_last_is_true_only_at_one_and_minus_one() {
        for (n, last) in [(1, true), (-1, true), (2, false), (-2, false), (0, false)] {
            assert_eq!(is_last(&AtomicI32::new(n)), last, "{n}");
        }
    }

    #[test]
    fn a_skipped_shared_dup_never_comes_back_to_the_decref_range() {
        // dup を1回でも飛ばした数は DUP_LIMIT より小さく、decref はもう数を動かさない
        let mut rc = DUP_LIMIT;
        assert_eq!(dup_step(rc), DupStep::Shared);
        rc -= 1;
        assert_eq!(dup_step(rc), DupStep::Skip);
        assert_eq!(decref_step(rc), DecrefStep::Skip);
    }
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_rt rc::`
Expected: `rc.rs` がないか関数がないので、コンパイルが失敗する

- [ ] **Step 3: 遷移を書く**

`crates/eml_rt/src/sync.rs` を次にする:

```rust
//! atomic とページの確保の層 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「`unsafe` と同期の層」)。
//! 後で `cfg(loom)` のときに loom の型へ差し替えるので、核はこの module から atomic を使う。

pub(crate) use std::sync::atomic::{AtomicI32, Ordering};
```

`crates/eml_rt/src/lib.rs` に `mod rc;` を足す (`mod mode;` の後)。

`crates/eml_rt/src/rc.rs` の本体:

```rust
//! RC の遷移 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「遷移の関数」)。rc の値だけを受けて次の
//! 状態を返す純粋な関数にし、ヒープ、単体テスト、Kani が同じ関数を使う。rc は 0 が不死、正が局所、負が共有である。

use crate::sync::{AtomicI32, Ordering};

/// 局所の数の上限。ここに達した数は動かさない (spec の「上限」)。
pub(crate) const LOCAL_MAX: i32 = i32::MAX;
/// 共有の `dup` は、これより小さい数を動かさない。
pub(crate) const DUP_LIMIT: i32 = i32::MIN + (1 << 16);
/// 共有の `decref` は、これより小さい数を動かさない。2つの境目の間の幅が、並行の加減算で数が戻ってくるのを防ぐ
/// (spec の「上限」)。
pub(crate) const DROP_LIMIT: i32 = i32::MIN + (1 << 17);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DupStep {
    /// 局所の数。この値を書き戻す。
    Store(i32),
    /// 共有の数。atomic に増やす。
    Shared,
    /// 不死か、張り付いた数。何もしない。
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DecrefStep {
    Store(i32),
    /// 局所の最後の参照。
    Free,
    Shared,
    Skip,
}

/// 局所の速い道は比較1回で決める。rc を符号なしで見て、1 以上 `LOCAL_MAX - 1` 以下かを1回で確かめる。
#[inline]
pub(crate) fn dup_step(rc: i32) -> DupStep {
    if (rc as u32).wrapping_sub(1) < (LOCAL_MAX - 1) as u32 {
        DupStep::Store(rc + 1)
    } else if (DUP_LIMIT..0).contains(&rc) {
        DupStep::Shared
    } else {
        DupStep::Skip
    }
}

/// 2 以上 `LOCAL_MAX - 1` 以下を比較1回で確かめ、1 なら解放する。
#[inline]
pub(crate) fn decref_step(rc: i32) -> DecrefStep {
    if (rc as u32).wrapping_sub(2) < (LOCAL_MAX - 2) as u32 {
        DecrefStep::Store(rc - 1)
    } else if rc == 1 {
        DecrefStep::Free
    } else if (DROP_LIMIT..0).contains(&rc) {
        DecrefStep::Shared
    } else {
        DecrefStep::Skip
    }
}

/// `share` の1物体分の印付け。局所の数 n を −n にし、`DROP_LIMIT` より小さくなるなら `MIN` に張り付ける。局所でない数は
/// たどりを止めるので `None` を返す。
pub(crate) fn mark_step(rc: i32) -> Option<i32> {
    if rc <= 0 {
        return None;
    }
    let shared = -rc;
    Some(if shared < DROP_LIMIT { i32::MIN } else { shared })
}

pub(crate) fn dup_shared(rc: &AtomicI32) {
    rc.fetch_sub(1, Ordering::Relaxed);
}

/// 共有の数を1つ減らし、最後の参照なら真を返す。`fence(Acquire)` を使わず、AcqRel の加算で最後の解放を順序付ける。
pub(crate) fn decref_shared(rc: &AtomicI32) -> bool {
    rc.fetch_add(1, Ordering::AcqRel) == -1
}

/// 手放そうとしている参照が最後のものか。rc は Relaxed で読み、−1 のときだけ Acquire で読み直す。ふだんの rc が 1 の
/// 道に Acquire の読み込みの費用を足さず、−1 のときは最後の AcqRel の `decref` と同期するためである。
pub(crate) fn is_last(rc: &AtomicI32) -> bool {
    match rc.load(Ordering::Relaxed) {
        1 => true,
        -1 => rc.load(Ordering::Acquire) == -1,
        _ => false,
    }
}
```

- [ ] **Step 4: テストを流す**

Run: `cargo test -p eml_rt rc::`
Expected: 6つのテストが通る

- [ ] **Step 5: fmt、clippy、コミット**

Run: `cargo fmt && cargo clippy -p eml_rt --all-targets` (`dead_code` だけを許す)

```bash
git add crates/eml_rt
git commit -m "Add the RC transitions of eml_rt

The fast paths decide with one unsigned comparison; local counts stick at
i32::MAX and shared counts use two thresholds so a skipped dup can never be
undone by in-flight decrefs.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YLHZowtbHVXEbsTMFYwSMN"
```

---

### Task 4: ページの確保器

**Files:**
- Create: `crates/eml_rt/src/alloc.rs`
- Modify: `crates/eml_rt/src/lib.rs` (`mod alloc;`)
- Modify: `crates/eml_rt/src/sync.rs` (確保の関数)

**Interfaces:**
- Consumes: `meta::{Header, Kind, Meta, init, meta, set_meta, slot}`
- Produces: `alloc::PAGE_BYTES`、`Pages::new(prefix_words: usize)`、`Pages::alloc(&mut self, words: usize) -> NonNull<Header>` (`words` はヘッダを含み前置きを含まない)、`unsafe Pages::free(&mut self, h)`、`Pages::bytes(&self) -> usize`、`unsafe Pages::block_words(&self, h) -> usize` (前置きを含む)、`unsafe Pages::block_bytes(&self, h) -> usize`、`unsafe Pages::fits(&self, h, words: usize) -> bool`、`Pages::for_each_block(&self, impl FnMut(NonNull<Header>))`

- [ ] **Step 1: テストを書く**

`crates/eml_rt/src/alloc.rs` の末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn init(h: NonNull<Header>) {
        unsafe { meta::init(h, 1, Meta::new(0, 0, Kind::Data, false)) }
    }

    fn blocks(pages: &Pages) -> usize {
        let mut count = 0;
        pages.for_each_block(|_| count += 1);
        count
    }

    #[test]
    fn the_class_table_rounds_up_to_the_next_class() {
        assert_eq!(class_of(0), Some(0));
        assert_eq!(class_of(2), Some(0));
        assert_eq!(class_of(3), Some(1));
        assert_eq!(class_of(16), Some(14));
        assert_eq!(CLASS_WORDS[class_of(17).unwrap()], 20);
        assert_eq!(class_of(128), Some(CLASSES - 1));
        assert_eq!(class_of(129), None);
    }

    #[test]
    fn blocks_of_one_class_are_carved_back_to_back() {
        let mut pages = Pages::new(0);
        let a = pages.alloc(3);
        let b = pages.alloc(3);
        assert_eq!(a.addr().get() % WORD, 0);
        assert_eq!(b.addr().get() - a.addr().get(), 3 * WORD);
        assert_eq!(unsafe { pages.block_words(a) }, 3);
        assert_eq!(pages.bytes(), PAGE_BYTES);
    }

    #[test]
    fn a_freed_block_is_marked_free_and_reused_first() {
        let mut pages = Pages::new(0);
        let a = pages.alloc(2);
        init(a);
        init(pages.alloc(2));
        unsafe { pages.free(a) };
        assert_eq!(unsafe { meta::meta(a) }.kind(), Kind::Free);
        assert_eq!(pages.alloc(2), a);
    }

    #[test]
    fn a_new_page_writes_back_where_the_old_one_stopped() {
        let mut pages = Pages::new(0);
        let per_page = (PAGE_BYTES - FIRST_BLOCK) / (16 * WORD);
        for _ in 0..per_page + 3 {
            init(pages.alloc(16));
        }
        assert_eq!(pages.list.len(), 2);
        assert_eq!(blocks(&pages), per_page + 3);
    }

    #[test]
    fn a_large_object_gets_its_own_page_and_is_returned_at_once() {
        let mut pages = Pages::new(0);
        init(pages.alloc(2));
        let [a, b, c] = [200, 300, 400].map(|words| {
            let h = pages.alloc(words);
            init(h);
            h
        });
        assert_eq!(pages.list.len(), 4);
        let before = pages.bytes();
        unsafe { pages.free(a) };
        assert_eq!(pages.list.len(), 3);
        assert_eq!(pages.bytes(), before - (FIRST_BLOCK + 200 * WORD));
        // a の添字に移った c も続けて返せる
        unsafe {
            pages.free(c);
            pages.free(b);
        }
        assert_eq!(pages.list.len(), 1);
        assert_eq!(blocks(&pages), 1);
    }

    #[test]
    fn the_prefix_shifts_the_header_and_the_class() {
        let mut pages = Pages::new(1);
        // 前置きを足して 17 語なので、20 語の級になる
        let a = pages.alloc(16);
        assert_eq!(unsafe { pages.block_words(a) }, 20);
        let offset = a.addr().get() - page_of(a).addr().get() - FIRST_BLOCK;
        assert_eq!(offset % (20 * WORD), WORD);
        assert!(unsafe { pages.fits(a, 16) });
        assert!(unsafe { pages.fits(a, 19) });
        assert!(!unsafe { pages.fits(a, 15) });
        assert!(!unsafe { pages.fits(a, 20) });
    }
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_rt alloc::`
Expected: コンパイルが失敗する

- [ ] **Step 3: 確保器を書く**

`crates/eml_rt/src/sync.rs` を次にする:

```rust
//! atomic とページの確保の層 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「`unsafe` と同期の層」)。
//! 後で `cfg(loom)` のときに loom の型へ差し替えるので、核はこの module から atomic と確保の関数を使う。

pub(crate) use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
pub(crate) use std::sync::atomic::{AtomicI32, Ordering};
```

`crates/eml_rt/src/lib.rs` に `mod alloc;` を足す (先頭の `mod` の並びの最初)。

`crates/eml_rt/src/alloc.rs` の本体:

```rust
//! ページ単位の確保器 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「ページ」と「確保と解放」)。物体は
//! 64 KiB に揃えたページから切り出し、大きさはヘッダでなくページの頭が持つ。ヘッダの中身はここでは読まない。

use std::num::NonZero;
use std::ptr::{self, NonNull};

use crate::meta::{self, Header, Kind, Meta};
use crate::sync::{Layout, alloc, dealloc, handle_alloc_error};

pub(crate) const PAGE_BYTES: usize = 1 << 16;
const WORD: usize = 8;

/// 小さい物体の級のブロックの語の数 (ヘッダと、検査モードでは前置きを含む)。2 語から 16 語は1語刻みにする。
const CLASS_WORDS: [usize; 27] = [
    2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128,
];
const CLASSES: usize = CLASS_WORDS.len();
const MAX_SMALL_WORDS: usize = 128;

/// ブロックの語の数から級を引く表。確保の速い道で級を探して回らないためである。
const CLASS_OF: [u8; MAX_SMALL_WORDS + 1] = {
    let mut table = [0; MAX_SMALL_WORDS + 1];
    let (mut words, mut class) = (0, 0);
    while words <= MAX_SMALL_WORDS {
        while CLASS_WORDS[class] < words {
            class += 1;
        }
        table[words] = class as u8;
        words += 1;
    }
    table
};

/// 大きい物体のページの級。
const LARGE: u32 = u32::MAX;

fn class_of(block_words: usize) -> Option<usize> {
    CLASS_OF.get(block_words).map(|&class| usize::from(class))
}

#[repr(C)]
struct PageHeader {
    class: u32,
    block_words: u32,
    /// `Pages::list` の中の添字。大きい物体のページを `swap_remove` で外すときに、移ってきたページの添字を直す。
    index: usize,
    /// 切り出した終わりの位置 (ページの先頭からのバイト数)。今のページではヒープの欄だけが正しく、ページを替える
    /// ときに書き戻す。
    limit: usize,
    /// `alloc` に渡した大きさ。
    bytes: usize,
}

const FIRST_BLOCK: usize = size_of::<PageHeader>().next_multiple_of(WORD);

#[derive(Clone, Copy)]
struct Class {
    /// 空いたブロックの LIFO の先頭。次のブロックは、空いたブロックのペイロードの先頭の語にある。
    free: *mut Header,
    page: *mut PageHeader,
    cursor: *mut u8,
    end: *mut u8,
}

impl Class {
    const EMPTY: Class = Class {
        free: ptr::null_mut(),
        page: ptr::null_mut(),
        cursor: ptr::null_mut(),
        end: ptr::null_mut(),
    };
}

pub(crate) struct Pages {
    prefix_words: usize,
    list: Vec<NonNull<PageHeader>>,
    classes: [Class; CLASSES],
    bytes: usize,
}

/// ブロックはどれもページの先頭から 64 KiB の内にあるので、下位 16 ビットを落とせばページの頭になる。
fn page_of(h: NonNull<Header>) -> NonNull<PageHeader> {
    h.cast::<u8>()
        .map_addr(|addr| NonZero::new(addr.get() & !(PAGE_BYTES - 1)).expect("a page is not at address 0"))
        .cast()
}

impl Pages {
    pub(crate) fn new(prefix_words: usize) -> Pages {
        Pages {
            prefix_words,
            list: Vec::new(),
            classes: [Class::EMPTY; CLASSES],
            bytes: 0,
        }
    }

    /// OS から取ったページのバイト数。
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    /// `words` 語 (ヘッダを含み、前置きを含まない) の物体のブロックを取り、ヘッダの位置を返す。ヘッダは書かない。
    pub(crate) fn alloc(&mut self, words: usize) -> NonNull<Header> {
        let block_words = words + self.prefix_words;
        match class_of(block_words) {
            Some(class) => self.alloc_small(class),
            None => self.alloc_large(block_words),
        }
    }

    fn alloc_small(&mut self, class: usize) -> NonNull<Header> {
        if let Some(h) = NonNull::new(self.classes[class].free) {
            self.classes[class].free = unsafe { meta::slot(h, 0).cast::<*mut Header>().read() };
            return h;
        }
        let block_bytes = CLASS_WORDS[class] * WORD;
        let state = self.classes[class];
        if state.cursor.is_null() || state.end.addr() - state.cursor.addr() < block_bytes {
            self.next_page(class);
        }
        let state = &mut self.classes[class];
        let block = state.cursor;
        state.cursor = unsafe { block.add(block_bytes) };
        unsafe { NonNull::new_unchecked(block.add(self.prefix_words * WORD)).cast() }
    }

    fn next_page(&mut self, class: usize) {
        let old = self.classes[class];
        if let Some(page) = NonNull::new(old.page) {
            unsafe { (*page.as_ptr()).limit = old.cursor.addr() - page.addr().get() };
        }
        let page = self.new_page(PAGE_BYTES, class as u32, CLASS_WORDS[class]);
        let base = page.as_ptr().cast::<u8>();
        let state = &mut self.classes[class];
        state.page = page.as_ptr();
        state.cursor = unsafe { base.add(FIRST_BLOCK) };
        state.end = unsafe { base.add(PAGE_BYTES) };
    }

    fn alloc_large(&mut self, block_words: usize) -> NonNull<Header> {
        let bytes = FIRST_BLOCK + block_words * WORD;
        let page = self.new_page(bytes, LARGE, block_words);
        unsafe { (*page.as_ptr()).limit = bytes };
        let block = unsafe { page.as_ptr().cast::<u8>().add(FIRST_BLOCK) };
        unsafe { NonNull::new_unchecked(block.add(self.prefix_words * WORD)).cast() }
    }

    fn new_page(&mut self, bytes: usize, class: u32, block_words: usize) -> NonNull<PageHeader> {
        let layout = Layout::from_size_align(bytes, PAGE_BYTES).expect("a page layout");
        let Some(page) = NonNull::new(unsafe { alloc(layout) }.cast::<PageHeader>()) else {
            handle_alloc_error(layout)
        };
        let header = PageHeader {
            class,
            block_words: u32::try_from(block_words).expect("an object of fewer than 2^32 words"),
            index: self.list.len(),
            limit: FIRST_BLOCK,
            bytes,
        };
        unsafe { page.as_ptr().write(header) };
        self.list.push(page);
        self.bytes += bytes;
        page
    }

    /// 速いモードで物体の箱を空ける。小さい物体のブロックは種類 `Free` の印と次のブロックを書いて空きリストに積む。
    /// 印を書くのは、速いモードでもページを歩いて生きている物体を見分けるためである (spec の「生存の列挙」)。
    /// 大きい物体のページはすぐ返す。
    ///
    /// # Safety
    /// `h` はこの `Pages` が確保した、まだ空けていないブロックのヘッダを指し、ペイロードは1語以上ある。
    pub(crate) unsafe fn free(&mut self, h: NonNull<Header>) {
        let page = page_of(h);
        let class = unsafe { (*page.as_ptr()).class };
        if class == LARGE {
            unsafe { self.release_large(page) };
            return;
        }
        let state = &mut self.classes[class as usize];
        unsafe {
            meta::set_meta(h, Meta::new(0, 0, Kind::Free, false));
            meta::slot(h, 0).cast::<*mut Header>().write(state.free);
        }
        state.free = h.as_ptr();
    }

    unsafe fn release_large(&mut self, page: NonNull<PageHeader>) {
        let (index, bytes) = unsafe { ((*page.as_ptr()).index, (*page.as_ptr()).bytes) };
        self.list.swap_remove(index);
        if let Some(moved) = self.list.get(index) {
            unsafe { (*moved.as_ptr()).index = index };
        }
        self.bytes -= bytes;
        unsafe { dealloc(page.as_ptr().cast(), Layout::from_size_align_unchecked(bytes, PAGE_BYTES)) };
    }

    /// ブロックの語の数 (前置きを含む)。
    ///
    /// # Safety
    /// `h` はこの `Pages` が確保したブロックのヘッダを指す。
    pub(crate) unsafe fn block_words(&self, h: NonNull<Header>) -> usize {
        unsafe { (*page_of(h).as_ptr()).block_words as usize }
    }

    /// ブロックが使うバイト数。大きい物体ではページ全体である。検査モードの隔離の数え方に使う。
    ///
    /// # Safety
    /// `h` はこの `Pages` が確保したブロックのヘッダを指す。
    pub(crate) unsafe fn block_bytes(&self, h: NonNull<Header>) -> usize {
        let page = page_of(h);
        unsafe {
            if (*page.as_ptr()).class == LARGE {
                (*page.as_ptr()).bytes
            } else {
                (*page.as_ptr()).block_words as usize * WORD
            }
        }
    }

    /// 箱が `words` 語の物体の級か (token の検査)。
    ///
    /// # Safety
    /// `h` はこの `Pages` が確保したブロックのヘッダを指す。
    pub(crate) unsafe fn fits(&self, h: NonNull<Header>, words: usize) -> bool {
        let block_words = words + self.prefix_words;
        let want = class_of(block_words).map_or(block_words, |class| CLASS_WORDS[class]);
        unsafe { self.block_words(h) == want }
    }

    /// 切り出したすべてのブロックのヘッダの位置を呼ぶ。空いたブロックも含むので、見分けるのは呼ぶ側である。各級の
    /// 今のページではヒープの切り出しの位置を読み、まだ切り出していないメモリをブロックとして読まない。
    pub(crate) fn for_each_block(&self, mut f: impl FnMut(NonNull<Header>)) {
        for &page in &self.list {
            let (class, block_words, limit) = unsafe {
                let header = &*page.as_ptr();
                (header.class, header.block_words as usize, header.limit)
            };
            let limit = match self.classes.get(class as usize) {
                Some(state) if state.page == page.as_ptr() => state.cursor.addr() - page.addr().get(),
                _ => limit,
            };
            let base = page.as_ptr().cast::<u8>();
            let block_bytes = block_words * WORD;
            let mut offset = FIRST_BLOCK;
            while offset + block_bytes <= limit {
                f(unsafe { NonNull::new_unchecked(base.add(offset + self.prefix_words * WORD)).cast() });
                offset += block_bytes;
            }
        }
    }
}

impl Drop for Pages {
    fn drop(&mut self) {
        for page in self.list.drain(..) {
            unsafe {
                let bytes = (*page.as_ptr()).bytes;
                dealloc(page.as_ptr().cast(), Layout::from_size_align_unchecked(bytes, PAGE_BYTES));
            }
        }
    }
}
```

- [ ] **Step 4: テストを流す**

Run: `cargo test -p eml_rt alloc::`
Expected: 6つのテストが通る

- [ ] **Step 5: fmt、clippy、コミット**

Run: `cargo fmt && cargo clippy -p eml_rt --all-targets` (`dead_code` だけを許す)

```bash
git add crates/eml_rt
git commit -m "Add the page allocator of eml_rt

Objects are carved from 64 KiB aligned pages whose header holds the size
class, so object headers need no size; small blocks go on per-class LIFO
free lists marked Free so pages can be walked, and large pages are
returned at once.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YLHZowtbHVXEbsTMFYwSMN"
```

---

### Task 5: ヒープと検査モード

**Files:**
- Create: `crates/eml_rt/src/error.rs`
- Create: `crates/eml_rt/src/foreign.rs`
- Create: `crates/eml_rt/src/heap/mod.rs`
- Create: `crates/eml_rt/src/heap/check.rs`
- Create: `crates/eml_rt/src/heap/walk.rs`
- Create: `crates/eml_rt/src/heap/tests/mod.rs`
- Create: `crates/eml_rt/src/heap/tests/basics.rs`
- Create: `crates/eml_rt/src/heap/tests/checked.rs`
- Modify: `crates/eml_rt/src/lib.rs`
- Modify: `crates/eml_rt/src/sync.rs` (`AtomicU32`)

**Interfaces:**
- Consumes: Task 2 から Task 4 のすべて
- Produces:
  - `HeapError` (下の Step 3 の全部の値。後のタスクの誤りも先に定める)
  - `Stats { rc_increments, rc_decrements, allocations, live, peak_live }` (`u64`、`#[non_exhaustive]`)
  - `Heap<M>`: `new`、`dummy`、`worker`、`set_worker`、`stats`、`bytes_in_use`、`alloc(payload_words, meta) -> Word`、`alloc_immortal(payload_words, meta) -> Word`、`alloc_data(tag, scanned, raw, lin) -> Word`、`alloc_int(i64) -> Word`、`alloc_foreign<T: Send + 'static>(T, lin) -> Word`
  - `unsafe` の操作 (どれも `Result<_, HeapError>`): `acquire_immortal`、`dup`、`decref`、`is_unique`、`is_last`、`free_block`、`meta`、`field`、`set_field`、`raw`、`set_raw`、`scanned_len`、`scanned`、`unbox_int`、`foreign::<T>`
  - `live_objects() -> Vec<Word>` (両方のモード)、`Heap<Checked>::leaks() -> BTreeMap<&'static str, u64>`
  - crate の中: `object_words`、`scan_count`、`scan_slot`、`Heap::{alloc_header, release_ref, drain, free_box, check_live, check_owner, check_bounds}`、`heap::check::{Prefix, State, PREFIX_WORDS, UNCOUNTED, prefix, init_prefix, count_immortal}`、フィールド `work`

- [ ] **Step 1: テストの骨組みを書く**

`crates/eml_rt/src/heap/tests/mod.rs`:

```rust
//! ヒープの単体テスト (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「核の操作」)。モードを型の引数に
//! 取る関数を `both_modes!` で2つのモードのテストにし、同じ列を速いモードと検査モードで流す。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{Heap, Mode, Word};

/// モードを型の引数に取るテストの関数を、速いモードと検査モードの2つのテストにする。
macro_rules! both_modes {
    ($($name:ident),* $(,)?) => {
        mod fast {
            $( #[test] fn $name() { super::$name::<crate::Fast>() } )*
        }
        mod checked {
            $( #[test] fn $name() { super::$name::<crate::Checked>() } )*
        }
    };
}

mod basics;
mod checked;

/// Miri で遅くなりすぎない長さ。
pub(super) const LONG: usize = if cfg!(miri) { 300 } else { 100_000 };

pub(super) const CONS: u16 = 1;

/// cons セル (要素, 残り)。
pub(super) fn cons<M: Mode>(heap: &mut Heap<M>, head: Word, tail: Word) -> Word {
    let cell = heap.alloc_data(CONS, 2, 0, false);
    unsafe {
        heap.set_field(cell, 0, head).unwrap();
        heap.set_field(cell, 1, tail).unwrap();
    }
    cell
}

/// 即値の要素の長さ n のリスト。
pub(super) fn list<M: Mode>(heap: &mut Heap<M>, n: usize) -> Word {
    (0..n).fold(Word::imm(0), |tail, i| cons(heap, Word::imm(i as i64), tail))
}

/// ダミーのほかに生きている箱の数。
pub(super) fn mortal<M: Mode>(heap: &Heap<M>) -> usize {
    heap.live_objects().len() - 1
}

/// 破棄処理が呼ばれた回数を数える `Foreign` の値。
pub(super) struct Probe(pub(super) Arc<AtomicUsize>);

impl Drop for Probe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

pub(super) fn probe() -> (Probe, Arc<AtomicUsize>) {
    let drops = Arc::new(AtomicUsize::new(0));
    (Probe(drops.clone()), drops)
}
```

- [ ] **Step 2: 両方のモードのテストを書く**

`crates/eml_rt/src/heap/tests/basics.rs`:

```rust
use std::sync::atomic::Ordering;

use super::{LONG, cons, list, mortal, probe};
use crate::{Fast, Heap, HeapError, Kind, Meta, Mode, Word};

fn a_cons_list_is_freed_by_one_decref<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let l = list(&mut heap, 3);
    assert_eq!(mortal(&heap), 3);
    unsafe { heap.decref(l) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn a_dupped_object_survives_one_decref<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let x = list(&mut heap, 1);
    unsafe {
        heap.dup(x).unwrap();
        assert!(!heap.is_unique(x).unwrap());
        heap.decref(x).unwrap();
        assert!(heap.is_unique(x).unwrap());
        assert_eq!(mortal(&heap), 1);
        heap.decref(x).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_long_list_with_boxed_elements_frees_with_a_short_worklist<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let mut l = Word::imm(0);
    for i in 0..LONG {
        let pair = heap.alloc_data(0, 2, 0, false);
        unsafe {
            heap.set_field(pair, 0, Word::imm(i as i64)).unwrap();
            heap.set_field(pair, 1, Word::imm(i as i64)).unwrap();
        }
        l = cons(&mut heap, pair, l);
    }
    unsafe { heap.decref(l) }.unwrap();
    assert_eq!(mortal(&heap), 0);
    // 要素を残りのリストより先に空けるので、作業リストは要素1つと残りの1つで足りる
    assert!(heap.work.capacity() <= 4, "{}", heap.work.capacity());
}

fn a_deep_chain_does_not_overflow_the_stack<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let mut x = Word::imm(0);
    for _ in 0..LONG {
        let cell = heap.alloc_data(0, 1, 0, false);
        unsafe { heap.set_field(cell, 0, x) }.unwrap();
        x = cell;
    }
    unsafe { heap.decref(x) }.unwrap();
    assert_eq!(mortal(&heap), 0);
}

fn a_big_int_is_boxed_only_outside_63_bits<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let small = heap.alloc_int(5);
    let big = heap.alloc_int(i64::MAX);
    assert!(small.is_imm());
    assert!(big.is_ptr());
    unsafe {
        assert_eq!(heap.unbox_int(small), Ok(5));
        assert_eq!(heap.unbox_int(big), Ok(i64::MAX));
        assert_eq!(heap.meta(big).unwrap().kind(), Kind::BigInt);
        heap.decref(big).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn the_dummy_ignores_dup_and_decref_and_unboxes_to_zero<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let dummy = heap.dummy();
    unsafe {
        for _ in 0..3 {
            heap.dup(dummy).unwrap();
        }
        for _ in 0..5 {
            heap.decref(dummy).unwrap();
        }
        assert_eq!(heap.unbox_int(dummy), Ok(0));
    }
    assert_eq!(heap.live_objects(), vec![dummy]);
}

fn an_extended_scan_object_frees_all_its_children<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let n = 300;
    let x = heap.alloc_data(0, n, 1, false);
    unsafe {
        assert_eq!(heap.meta(x).unwrap().scan(), Meta::EXTENDED_SCAN);
        assert_eq!(heap.scanned_len(x), Ok(n));
        for i in 0..n {
            // 物理的な添字は、数の語の次から始まる
            let child = list(&mut heap, 1);
            heap.set_field(x, 1 + i, child).unwrap();
            assert_eq!(heap.scanned(x, i), Ok(child));
        }
        heap.set_raw(x, 1 + n, 7).unwrap();
        assert_eq!(heap.raw(x, 1 + n), Ok(7));
        heap.decref(x).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_foreign_value_is_finalized_exactly_once<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let (value, drops) = probe();
    let f = heap.alloc_foreign(value, true);
    let holder = heap.alloc_data(0, 1, 0, false);
    unsafe {
        assert!(heap.meta(f).unwrap().lin());
        heap.set_field(holder, 0, f).unwrap();
        heap.decref(holder).unwrap();
    }
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    drop(heap);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

fn dropping_the_heap_finalizes_live_foreign_values<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let (value, drops) = probe();
    heap.alloc_foreign(value, false);
    drop(heap);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

fn a_zero_sized_foreign_value_fits<M: Mode>() {
    struct Nothing;
    let mut heap = Heap::<M>::new();
    let f = heap.alloc_foreign(Nothing, false);
    unsafe {
        assert!(heap.foreign::<Nothing>(f).is_ok());
        assert_eq!(heap.foreign::<u8>(f), Err(HeapError::WrongKind));
        heap.decref(f).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn is_last_and_free_block_free_a_known_shape<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let x = heap.alloc_data(0, 0, 1, false);
    unsafe {
        assert!(heap.is_last(x).unwrap());
        heap.dup(x).unwrap();
        assert!(!heap.is_last(x).unwrap());
        heap.decref(x).unwrap();
        heap.free_block(x).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn bytes_in_use_come_back_after_a_large_object_dies<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let before = heap.bytes_in_use();
    let x = heap.alloc_data(0, 0, 500, false);
    assert!(heap.bytes_in_use() > before);
    unsafe { heap.decref(x) }.unwrap();
    assert_eq!(heap.bytes_in_use(), before);
}

fn immortal_objects_are_never_freed<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let x = heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, false));
    unsafe {
        heap.acquire_immortal(x).unwrap();
        heap.dup(x).unwrap();
        heap.decref(x).unwrap();
        heap.decref(x).unwrap();
    }
    assert_eq!(mortal(&heap), 1);
}

fn the_boundary_between_small_and_large_works_in_both_modes<M: Mode>() {
    let mut heap = Heap::<M>::new();
    // 127 語のペイロードとヘッダで 128 語。検査モードでは前置きを足して大きい物体の道に移る
    for raw in [126, 127, 128] {
        let x = heap.alloc_data(0, 0, raw, false);
        unsafe {
            heap.set_raw(x, raw - 1, 9).unwrap();
            assert_eq!(heap.raw(x, raw - 1), Ok(9));
        }
        assert_eq!(mortal(&heap), 1);
        unsafe { heap.decref(x) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn mixed_sizes_keep_the_free_lists_apart<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let mut live: Vec<(Word, u64)> = Vec::new();
    let mut seed: u64 = 1;
    let rounds = if cfg!(miri) { 200 } else { 20_000 };
    for id in 0..rounds {
        // 線形合同法で大きさと、確保か解放かを決める
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        if seed >> 62 == 0 && !live.is_empty() {
            let (w, _) = live.swap_remove((seed >> 8) as usize % live.len());
            unsafe { heap.decref(w) }.unwrap();
        } else {
            let raw = 1 + (seed >> 40) as usize % 40;
            let w = heap.alloc_data(0, 0, raw, false);
            unsafe { heap.set_raw(w, 0, id) }.unwrap();
            live.push((w, id));
        }
    }
    assert_eq!(mortal(&heap), live.len());
    for (w, id) in live {
        assert_eq!(unsafe { heap.raw(w, 0) }, Ok(id));
        unsafe { heap.decref(w) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn walking_counts_objects_across_pages<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let objects: Vec<Word> = (0..2000).map(|_| heap.alloc_data(0, 0, 15, false)).collect();
    assert_eq!(mortal(&heap), 2000);
    for w in objects {
        unsafe { heap.decref(w) }.unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

both_modes!(
    a_cons_list_is_freed_by_one_decref,
    a_dupped_object_survives_one_decref,
    a_long_list_with_boxed_elements_frees_with_a_short_worklist,
    a_deep_chain_does_not_overflow_the_stack,
    a_big_int_is_boxed_only_outside_63_bits,
    the_dummy_ignores_dup_and_decref_and_unboxes_to_zero,
    an_extended_scan_object_frees_all_its_children,
    a_foreign_value_is_finalized_exactly_once,
    dropping_the_heap_finalizes_live_foreign_values,
    a_zero_sized_foreign_value_fits,
    is_last_and_free_block_free_a_known_shape,
    bytes_in_use_come_back_after_a_large_object_dies,
    immortal_objects_are_never_freed,
    the_boundary_between_small_and_large_works_in_both_modes,
    mixed_sizes_keep_the_free_lists_apart,
    walking_counts_objects_across_pages,
);

#[test]
fn the_fast_heap_reuses_a_freed_block() {
    let mut heap = Heap::<Fast>::new();
    let a = heap.alloc_data(0, 0, 1, false);
    unsafe { heap.decref(a) }.unwrap();
    assert_eq!(heap.alloc_data(0, 0, 1, false), a);
}
```

- [ ] **Step 3: 検査モードのテストを書く**

`crates/eml_rt/src/heap/tests/checked.rs`:

```rust
use std::collections::BTreeMap;

use super::{list, probe};
use crate::{Checked, Heap, HeapError, Kind, Meta, Word};

type H = Heap<Checked>;

#[test]
fn using_a_freed_object_is_an_error() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.decref(x).unwrap();
        assert_eq!(heap.dup(x), Err(HeapError::UseAfterFree));
        assert_eq!(heap.field(x, 0), Err(HeapError::UseAfterFree));
        assert_eq!(heap.decref(x), Err(HeapError::UseAfterFree));
        assert_eq!(heap.is_unique(x), Err(HeapError::UseAfterFree));
    }
}

#[test]
fn the_checked_heap_never_reuses_a_freed_block() {
    let mut heap = H::new();
    let a = heap.alloc_data(0, 0, 1, false);
    unsafe { heap.decref(a) }.unwrap();
    assert_ne!(heap.alloc_data(0, 0, 1, false), a);
}

#[test]
fn dupping_a_lin_object_is_an_error() {
    let mut heap = H::new();
    let f = heap.alloc_data(0, 0, 1, true);
    unsafe {
        assert_eq!(heap.dup(f), Err(HeapError::LinDup));
        heap.decref(f).unwrap();
    }
}

#[test]
fn another_worker_cannot_touch_a_local_object() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 1, 0, false);
    heap.set_worker(1);
    let wrong = Err(HeapError::WrongOwner { owner: 0, worker: 1 });
    unsafe {
        assert_eq!(heap.dup(x), wrong);
        assert_eq!(heap.is_unique(x).map(|_| ()), wrong);
        assert_eq!(heap.set_field(x, 0, Word::UNIT), wrong);
        heap.set_worker(0);
        heap.decref(x).unwrap();
    }
}

#[test]
fn an_immortal_object_counts_its_outside_references() {
    let mut heap = H::new();
    let x = heap.alloc_immortal(1, Meta::new(0, 1, Kind::Data, false));
    unsafe {
        // 作るときのフィールドの書き込みは数を見ない
        heap.set_field(x, 0, Word::UNIT).unwrap();
        assert_eq!(heap.decref(x), Err(HeapError::ImmortalOverRelease));
        heap.acquire_immortal(x).unwrap();
        assert_eq!(heap.leaks(), BTreeMap::from([("Data", 1)]));
        heap.decref(x).unwrap();
    }
    assert!(heap.leaks().is_empty());
}

#[test]
fn a_borrowed_child_of_an_immortal_object_can_be_dupped() {
    let mut heap = H::new();
    let child = heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, false));
    let parent = heap.alloc_immortal(1, Meta::new(0, 1, Kind::Data, false));
    unsafe {
        heap.set_field(parent, 0, child).unwrap();
        let borrowed = heap.field(parent, 0).unwrap();
        heap.dup(borrowed).unwrap();
        heap.decref(borrowed).unwrap();
    }
    assert!(heap.leaks().is_empty());
}

#[test]
fn acquire_on_a_mortal_object_is_an_error() {
    let mut heap = H::new();
    let x = list(&mut heap, 1);
    unsafe {
        assert_eq!(heap.acquire_immortal(x), Err(HeapError::NotImmortal));
        heap.decref(x).unwrap();
    }
}

#[test]
fn leaks_are_counted_by_kind() {
    let mut heap = H::new();
    let a = list(&mut heap, 1);
    let b = heap.alloc_int(i64::MAX);
    let (value, _) = probe();
    let f = heap.alloc_foreign(value, false);
    assert_eq!(heap.leaks(), BTreeMap::from([("BigInt", 1), ("Data", 1), ("Foreign", 1)]));
    unsafe {
        for w in [a, b, f] {
            heap.decref(w).unwrap();
        }
    }
    assert!(heap.leaks().is_empty());
}

#[test]
fn the_dummy_is_never_a_leak() {
    let mut heap = H::new();
    unsafe { heap.dup(heap.dummy()) }.unwrap();
    assert!(heap.leaks().is_empty());
}

#[test]
fn stats_count_rc_operations_and_live_boxes() {
    let mut heap = H::new();
    let before = heap.stats();
    let x = list(&mut heap, 1);
    unsafe {
        heap.dup(x).unwrap();
        heap.decref(x).unwrap();
        heap.decref(x).unwrap();
    }
    let after = heap.stats();
    assert_eq!(after.rc_increments - before.rc_increments, 1);
    assert_eq!(after.rc_decrements - before.rc_decrements, 2);
    assert_eq!(after.allocations - before.allocations, 1);
    assert_eq!(after.live, before.live);
    assert_eq!(after.peak_live, before.live + 1);
}

#[test]
fn a_field_index_past_the_block_is_an_error() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 1, 0, false);
    unsafe {
        assert_eq!(heap.field(x, 1), Err(HeapError::OutOfBounds));
        heap.decref(x).unwrap();
    }
}

#[test]
fn free_block_refuses_a_finalizer_and_a_shared_count() {
    let mut heap = H::new();
    let (value, _) = probe();
    let f = heap.alloc_foreign(value, false);
    let x = heap.alloc_data(0, 0, 1, false);
    unsafe {
        assert_eq!(heap.free_block(f), Err(HeapError::FinalizerSkipped));
        heap.dup(x).unwrap();
        assert_eq!(heap.free_block(x), Err(HeapError::NotLast));
        heap.decref(x).unwrap();
        heap.decref(x).unwrap();
        heap.decref(f).unwrap();
    }
}
```

- [ ] **Step 4: テストが失敗することを確かめる**

`crates/eml_rt/src/lib.rs` に `mod error; mod foreign; mod heap;` を足し、`pub use error::HeapError; pub use heap::{Heap, Stats};` を足してから流す。

Run: `cargo test -p eml_rt heap::`
Expected: `error.rs`、`foreign.rs`、`heap/` がないので、コンパイルが失敗する

- [ ] **Step 5: 誤りと Foreign を書く**

`crates/eml_rt/src/error.rs`:

```rust
//! 検査モードが報告する誤り (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「報告する誤り」)。速いモードは
//! どれも返さない。

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeapError {
    UseAfterFree,
    ImmortalOverRelease,
    NotImmortal,
    LinDup,
    LinShared,
    LinReachedTwice,
    WrongOwner { owner: u32, worker: u32 },
    NotTokenable,
    NotUnique,
    NotLast,
    StaleToken,
    WrongClass,
    WrongMeta,
    LinBitKept,
    FinalizerSkipped,
    WrongLayout,
    WrongKind,
    OutOfBounds,
    SharedReachesLocal,
    ImmortalReachesMortal,
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HeapError::UseAfterFree => f.write_str("use of a freed object"),
            HeapError::ImmortalOverRelease => {
                f.write_str("an immortal object is released more often than it was acquired")
            }
            HeapError::NotImmortal => f.write_str("an acquire of an object that is not immortal"),
            // `eml_runtime` の `File` の検査と同じ文言にする
            HeapError::LinDup => f.write_str("a Lin object is shared"),
            HeapError::LinShared => f.write_str("a Lin object is on the shared side"),
            HeapError::LinReachedTwice => f.write_str("share reaches the same Lin object twice"),
            HeapError::WrongOwner { owner, worker } => {
                write!(f, "an object of worker {owner} is used by worker {worker}")
            }
            HeapError::NotTokenable => f.write_str("an object of this kind cannot become a reuse token"),
            HeapError::NotUnique => f.write_str("a reuse token is made from an object that is not unique"),
            HeapError::NotLast => f.write_str("free_block on an object that is still referenced"),
            HeapError::StaleToken => f.write_str("a reuse token is used twice or is stale"),
            HeapError::WrongClass => f.write_str("a reuse token has another size class"),
            HeapError::WrongMeta => f.write_str("reuse_in_place names another constructor"),
            HeapError::LinBitKept => {
                f.write_str("reuse_in_place keeps a lin bit the new object does not have")
            }
            HeapError::FinalizerSkipped => f.write_str("free_block on an object with a finalizer"),
            // `eml_runtime` の `release_fields` と同じ文言にする
            HeapError::WrongLayout => {
                f.write_str("an object does not have the tag and number of fields a release names")
            }
            HeapError::WrongKind => f.write_str("an object is not of the expected kind"),
            HeapError::OutOfBounds => f.write_str("a field index is past the end of the object"),
            HeapError::SharedReachesLocal => f.write_str("a shared object reaches a local object"),
            HeapError::ImmortalReachesMortal => f.write_str("an immortal object reaches a mortal object"),
        }
    }
}

impl std::error::Error for HeapError {}
```

`crates/eml_rt/src/foreign.rs`:

```rust
//! `Foreign` の物体 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「種類」)。ペイロードの先頭に破棄
//! 処理の表への参照を置き、その後に Rust の値を置く。

use std::any::TypeId;
use std::marker::PhantomData;
use std::ptr::NonNull;

use crate::meta::{self, Header};

pub(crate) struct VTable {
    /// 型の照らし合わせに使う。表の番地は型ごとに1つと限らないので、番地では比べない。
    type_id: fn() -> TypeId,
    finalize: unsafe fn(*mut u8),
}

struct Table<T>(PhantomData<T>);

impl<T: Send + 'static> Table<T> {
    const VTABLE: VTable = VTable {
        type_id: TypeId::of::<T>,
        finalize: drop_value::<T>,
    };
}

unsafe fn drop_value<T>(value: *mut u8) {
    unsafe { value.cast::<T>().drop_in_place() }
}

/// # Safety
/// `h` は種類 `Foreign` の初期化したヘッダを指し、ペイロードは表の1語と `T` を入れられる。
pub(crate) unsafe fn write<T: Send + 'static>(h: NonNull<Header>, value: T) {
    let table: &'static VTable = &Table::<T>::VTABLE;
    unsafe {
        meta::slot(h, 0).cast::<&'static VTable>().write(table);
        meta::slot(h, 1).cast::<T>().write(value);
    }
}

/// 値が `T` なら、その位置を返す。
///
/// # Safety
/// `h` は生きている `Foreign` の物体を指す。
pub(crate) unsafe fn value<T: 'static>(h: NonNull<Header>) -> Option<*mut T> {
    let table = unsafe { meta::slot(h, 0).cast::<&'static VTable>().read() };
    ((table.type_id)() == TypeId::of::<T>()).then(|| unsafe { meta::slot(h, 1) }.cast())
}

/// # Safety
/// `h` は生きている `Foreign` の物体を指し、破棄処理をまだ呼んでいない。
pub(crate) unsafe fn finalize(h: NonNull<Header>) {
    unsafe {
        let table = meta::slot(h, 0).cast::<&'static VTable>().read();
        (table.finalize)(meta::slot(h, 1).cast());
    }
}
```

- [ ] **Step 6: 検査モードの前置きを書く**

`crates/eml_rt/src/sync.rs` の atomic の行を `pub(crate) use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};` にする。

`crates/eml_rt/src/heap/check.rs`:

```rust
//! 検査モードの前置きと検査 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「前置き」と「報告する
//! 誤り」)。前置きは検査モードのブロックにだけあるので、前置きを読む関数は `M::CHECKED` のときだけ働く。

use std::ptr::NonNull;

use super::Heap;
use crate::error::HeapError;
use crate::meta::{self, HEADER_WORDS, Header};
use crate::mode::Mode;
use crate::sync::{AtomicU32, Ordering};

#[repr(C)]
pub(crate) struct Prefix {
    /// 普通の物体では所有者 ID、`alloc_immortal` の物体では外に出ている参照の数 (spec の「不死の物体の数え方」)。
    pub(crate) first: AtomicU32,
    /// 下位2ビットが状態、残りが世代の番号。
    state: AtomicU32,
}

pub(crate) const PREFIX_WORDS: usize = size_of::<Prefix>().div_ceil(8);
/// `first` のこの値の不死の物体 (ダミー) は、数を変えも確かめもしない。
pub(crate) const UNCOUNTED: u32 = u32::MAX;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Alive = 1,
    Token = 2,
    Freed = 3,
}

impl Prefix {
    pub(crate) fn state(&self) -> State {
        match self.state.load(Ordering::Relaxed) & 3 {
            1 => State::Alive,
            2 => State::Token,
            3 => State::Freed,
            bits => unreachable!("an unknown block state {bits}"),
        }
    }

    pub(crate) fn generation(&self) -> u32 {
        self.state.load(Ordering::Relaxed) >> 2
    }

    /// 状態を替える。`advance` なら世代の番号を1つ進める (`into_token` と解放)。
    pub(crate) fn set_state(&self, state: State, advance: bool) {
        let generation = self.generation().wrapping_add(u32::from(advance));
        self.state.store((generation << 2) | state as u32, Ordering::Relaxed);
    }
}

/// # Safety
/// `h` は検査モードのヒープが確保したブロックのヘッダを指す。
pub(crate) unsafe fn prefix<'a>(h: NonNull<Header>) -> &'a Prefix {
    unsafe { &*h.as_ptr().cast::<u8>().sub(PREFIX_WORDS * 8).cast::<Prefix>() }
}

/// # Safety
/// `h` は検査モードのヒープが今確保したブロックのヘッダを指す。
pub(crate) unsafe fn init_prefix(h: NonNull<Header>, first: u32) {
    let prefix = Prefix {
        first: AtomicU32::new(first),
        state: AtomicU32::new(State::Alive as u32),
    };
    unsafe { h.as_ptr().cast::<u8>().sub(PREFIX_WORDS * 8).cast::<Prefix>().write(prefix) }
}

/// 不死の物体の外に出ている参照の数を1つ増やすか減らす。数えた参照なら真を返す。
///
/// # Safety
/// `h` は検査モードの生きている不死の物体を指す。
pub(crate) unsafe fn count_immortal(h: NonNull<Header>, up: bool) -> Result<bool, HeapError> {
    let first = &unsafe { prefix(h) }.first;
    let n = first.load(Ordering::Relaxed);
    if n == UNCOUNTED {
        return Ok(false);
    }
    if up {
        first.store(n + 1, Ordering::Relaxed);
    } else if n == 0 {
        return Err(HeapError::ImmortalOverRelease);
    } else {
        first.store(n - 1, Ordering::Relaxed);
    }
    Ok(true)
}

impl<M: Mode> Heap<M> {
    /// 前置きの状態が生存かを確かめる。解放した共有の物体は rc が 0 になっていて不死の物体と区別できないので、rc を
    /// 見る前に呼ぶ。
    pub(super) unsafe fn check_live(&self, h: NonNull<Header>) -> Result<(), HeapError> {
        if !M::CHECKED || unsafe { prefix(h) }.state() == State::Alive {
            Ok(())
        } else {
            Err(HeapError::UseAfterFree)
        }
    }

    /// 正の rc の物体は、今の worker が持ち主でなければならない (spec の「論理的な worker ID」)。
    pub(super) unsafe fn check_owner(&self, h: NonNull<Header>, rc: i32) -> Result<(), HeapError> {
        if !M::CHECKED || rc <= 0 {
            return Ok(());
        }
        let owner = unsafe { prefix(h) }.first.load(Ordering::Relaxed);
        if owner == self.worker {
            Ok(())
        } else {
            Err(HeapError::WrongOwner { owner, worker: self.worker })
        }
    }

    pub(super) unsafe fn check_bounds(&self, h: NonNull<Header>, i: usize) -> Result<(), HeapError> {
        if !M::CHECKED {
            return Ok(());
        }
        let capacity = unsafe { self.pages.block_words(h) } - PREFIX_WORDS - HEADER_WORDS;
        if i < capacity { Ok(()) } else { Err(HeapError::OutOfBounds) }
    }

    /// `dup` の検査。数える参照なら真を返す。
    pub(super) unsafe fn check_dup(&self, h: NonNull<Header>) -> Result<bool, HeapError> {
        if !M::CHECKED {
            return Ok(false);
        }
        unsafe { self.check_live(h)? };
        if unsafe { meta::meta(h) }.lin() {
            return Err(HeapError::LinDup);
        }
        let rc = unsafe { meta::rc(h) }.load(Ordering::Relaxed);
        if rc == 0 {
            return unsafe { count_immortal(h, true) };
        }
        unsafe { self.check_owner(h, rc)? };
        Ok(true)
    }

    /// `decref` の検査。数える参照なら真を返す。
    pub(super) unsafe fn check_decref(&self, h: NonNull<Header>) -> Result<bool, HeapError> {
        if !M::CHECKED {
            return Ok(false);
        }
        unsafe { self.check_live(h)? };
        let rc = unsafe { meta::rc(h) }.load(Ordering::Relaxed);
        if rc == 0 {
            return unsafe { count_immortal(h, false) };
        }
        if rc < 0 && unsafe { meta::meta(h) }.lin() {
            return Err(HeapError::LinShared);
        }
        unsafe { self.check_owner(h, rc)? };
        Ok(true)
    }
}
```

- [ ] **Step 7: ヒープを書く**

`crates/eml_rt/src/heap/mod.rs`:

```rust
//! ヒープ: 確保、RC、解放の連鎖、フィールドの読み書き
//! (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「核の操作」)。語を受ける操作は `unsafe fn` で、
//! 安全の条件は spec の「核の操作」にある。検査モードは、確かめられる違反を `Err` で返す。

pub(crate) mod check;
mod walk;

#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::marker::PhantomData;
use std::mem;
use std::ptr::NonNull;

use crate::alloc::Pages;
use crate::error::HeapError;
use crate::foreign;
use crate::meta::{self, HEADER_WORDS, Header, Kind, Meta};
use crate::mode::Mode;
use crate::rc::{self, DecrefStep, DupStep};
use crate::sync::Ordering;
use crate::word::Word;

use check::{PREFIX_WORDS, State};

/// 検査モードだけで数える仕事の回数 (spec の「モードの選び方」)。ダミーの1つも数に入る。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stats {
    pub rc_increments: u64,
    pub rc_decrements: u64,
    pub allocations: u64,
    /// 生きている箱 (token の箱を含む) の数。
    pub live: u64,
    pub peak_live: u64,
}

pub struct Heap<M: Mode> {
    pages: Pages,
    worker: u32,
    /// 解放の連鎖の作業リスト。使い回して、解放のたびに確保しない。
    work: Vec<Word>,
    dummy: Word,
    /// 検査モードで隔離した (使い回さない) ブロックのバイト数。
    quarantined: usize,
    stats: Stats,
    _mode: PhantomData<(M, Cell<()>)>,
}

// `Heap` は worker ごとに1つ持ち、worker のスレッドに移せる。`Sync` にはしない (spec の「語」)
unsafe impl<M: Mode> Send for Heap<M> {}

impl<M: Mode> Default for Heap<M> {
    fn default() -> Heap<M> {
        Heap::new()
    }
}

/// ペイロードの語の数から、ヘッダを含む物体の語の数を出す。空いたブロックのペイロードの先頭に空きリストのポインタを
/// 置くので、ペイロードは少なくとも1語にする。
fn object_words(payload_words: usize) -> usize {
    HEADER_WORDS + payload_words.max(1)
}

/// 走査する語の数。`scan` が 255 なら、走査の始まりの生の語が数を持つ。
unsafe fn scan_count(h: NonNull<Header>) -> usize {
    let m = unsafe { meta::meta(h) };
    if m.scan() == Meta::EXTENDED_SCAN {
        unsafe { meta::slot(h, m.kind().scan_start()).cast::<u64>().read() as usize }
    } else {
        usize::from(m.scan())
    }
}

/// 走査する語の i 番目の位置。
unsafe fn scan_slot(h: NonNull<Header>, i: usize) -> *mut Word {
    let m = unsafe { meta::meta(h) };
    let start = m.kind().scan_start() + usize::from(m.scan() == Meta::EXTENDED_SCAN);
    unsafe { meta::slot(h, start + i) }
}

impl<M: Mode> Heap<M> {
    pub fn new() -> Heap<M> {
        let mut heap = Heap {
            pages: Pages::new(if M::CHECKED { PREFIX_WORDS } else { 0 }),
            worker: 0,
            work: Vec::new(),
            dummy: Word::EMPTY,
            quarantined: 0,
            stats: Stats::default(),
            _mode: PhantomData,
        };
        // ダミーは値 0 の BigInt の不死の物体で、検査モードでも数えない (spec の「ダミーの値」)
        let h = heap.alloc_header(1, 0, Meta::new(0, 0, Kind::BigInt, false));
        unsafe { meta::slot(h, 0).cast::<i64>().write(0) };
        if M::CHECKED {
            unsafe { check::prefix(h) }.first.store(check::UNCOUNTED, Ordering::Relaxed);
        }
        heap.dummy = Word::from_header(h);
        heap
    }

    pub fn dummy(&self) -> Word {
        self.dummy
    }

    pub fn worker(&self) -> u32 {
        self.worker
    }

    /// 論理的な worker ID を替える。これから確保する物体の所有者と、所有者の検査の相手になる。
    pub fn set_worker(&mut self, worker: u32) {
        self.worker = worker;
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// 使っているメモリのバイト数 (spec の「確保と解放」)。検査モードでは隔離したブロックを引く。
    pub fn bytes_in_use(&self) -> usize {
        self.pages.bytes() - self.quarantined
    }

    /// ブロックを取り、ヘッダと (検査モードでは) 前置きを書く。ペイロードは書かない。
    fn alloc_header(&mut self, payload_words: usize, rc: i32, meta: Meta) -> NonNull<Header> {
        debug_assert_ne!(meta.kind(), Kind::Free);
        let h = self.pages.alloc(object_words(payload_words));
        unsafe { meta::init(h, rc, meta) };
        if M::CHECKED {
            unsafe { check::init_prefix(h, if rc == 0 { 0 } else { self.worker }) };
            self.stats.allocations += 1;
            self.stats.live += 1;
            self.stats.peak_live = self.stats.peak_live.max(self.stats.live);
        }
        h
    }

    /// 物体を確保する。ペイロードは呼ぶ側が書く。
    pub fn alloc(&mut self, payload_words: usize, meta: Meta) -> Word {
        Word::from_header(self.alloc_header(payload_words, 1, meta))
    }

    /// 不死の物体を確保する。検査モードでは、外に出ている参照の数を 0 にして作る。
    pub fn alloc_immortal(&mut self, payload_words: usize, meta: Meta) -> Word {
        Word::from_header(self.alloc_header(payload_words, 0, meta))
    }

    /// データの物体を作る。走査する語が 255 個以上なら、ペイロードの先頭に数の語を置く。走査する語は、書く前に
    /// 解放の連鎖や `share` が読んでも子をたどらないように、`()` で埋めておく。
    pub fn alloc_data(&mut self, tag: u16, scanned: usize, raw: usize, lin: bool) -> Word {
        let extended = scanned >= usize::from(Meta::EXTENDED_SCAN);
        let scan = if extended { Meta::EXTENDED_SCAN } else { scanned as u8 };
        let h = self.alloc_header(usize::from(extended) + scanned + raw, 1, Meta::new(tag, scan, Kind::Data, lin));
        unsafe {
            if extended {
                meta::slot(h, 0).cast::<u64>().write(scanned as u64);
            }
            for i in 0..scanned {
                scan_slot(h, i).write(Word::UNIT);
            }
        }
        Word::from_header(h)
    }

    /// `Int` を `tobj` に入れる。63 ビットに収まれば即値、収まらなければ `BigInt` の物体にする。
    pub fn alloc_int(&mut self, n: i64) -> Word {
        Word::int(n).unwrap_or_else(|| {
            let h = self.alloc_header(1, 1, Meta::new(0, 0, Kind::BigInt, false));
            unsafe { meta::slot(h, 0).cast::<i64>().write(n) };
            Word::from_header(h)
        })
    }

    pub fn alloc_foreign<T: Send + 'static>(&mut self, value: T, lin: bool) -> Word {
        const { assert!(align_of::<T>() <= 8, "a foreign value must be at most word-aligned") };
        let h = self.alloc_header(1 + size_of::<T>().div_ceil(8), 1, Meta::new(0, 0, Kind::Foreign, lin));
        unsafe { foreign::write(h, value) };
        Word::from_header(h)
    }

    /// 不死の物体の参照を1つ作る (spec の「不死の物体の数え方」)。速いモードでは何もしない。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指す。
    pub unsafe fn acquire_immortal(&mut self, w: Word) -> Result<(), HeapError> {
        if M::CHECKED && w.is_ptr() {
            let h = w.header();
            unsafe { self.check_live(h)? };
            if unsafe { meta::rc(h) }.load(Ordering::Relaxed) != 0 {
                return Err(HeapError::NotImmortal);
            }
            if unsafe { check::count_immortal(h, true)? } {
                self.stats.rc_increments += 1;
            }
        }
        Ok(())
    }

    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指し、呼ぶ側が参照を持っているか、生きている持ち主から借りている。
    pub unsafe fn dup(&mut self, w: Word) -> Result<(), HeapError> {
        if !w.is_ptr() {
            return Ok(());
        }
        let h = w.header();
        let counted = unsafe { self.check_dup(h)? };
        let rc = unsafe { meta::rc(h) };
        match rc::dup_step(rc.load(Ordering::Relaxed)) {
            DupStep::Store(n) => rc.store(n, Ordering::Relaxed),
            DupStep::Shared => rc::dup_shared(rc),
            DupStep::Skip => {}
        }
        if counted {
            self.stats.rc_increments += 1;
        }
        Ok(())
    }

    /// 参照を1つ手放す。LIR の `decref` と `drop` は、どちらもこれで実行する (spec の「核の操作」)。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指し、呼ぶ側がその参照を1つ持っている。
    pub unsafe fn decref(&mut self, w: Word) -> Result<(), HeapError> {
        if !w.is_ptr() {
            return Ok(());
        }
        if unsafe { self.release_ref(w.header())? } {
            unsafe { self.free_chain(w.header())? };
        }
        Ok(())
    }

    /// 参照を1つ手放し、最後の参照なら真を返す。箱はまだ空けない。
    unsafe fn release_ref(&mut self, h: NonNull<Header>) -> Result<bool, HeapError> {
        let counted = unsafe { self.check_decref(h)? };
        let rc = unsafe { meta::rc(h) };
        let last = match rc::decref_step(rc.load(Ordering::Relaxed)) {
            DecrefStep::Store(n) => {
                rc.store(n, Ordering::Relaxed);
                false
            }
            DecrefStep::Free => true,
            DecrefStep::Shared => rc::decref_shared(rc),
            DecrefStep::Skip => false,
        };
        if counted {
            self.stats.rc_decrements += 1;
        }
        Ok(last)
    }

    unsafe fn free_chain(&mut self, root: NonNull<Header>) -> Result<(), HeapError> {
        let mut work = mem::take(&mut self.work);
        work.push(Word::from_header(root));
        let result = unsafe { self.drain(&mut work) };
        work.clear();
        self.work = work;
        result
    }

    /// 最後の参照を失った物体を空け、それで最後の参照を失った子を作業リストに積む。子は走査の逆の順に手放す。最初の
    /// フィールドを先に、最後のフィールド (cons セルの残りのリスト) を後に空けて、作業リストを短く保つためである。
    unsafe fn drain(&mut self, work: &mut Vec<Word>) -> Result<(), HeapError> {
        while let Some(w) = work.pop() {
            let h = w.header();
            for i in (0..unsafe { scan_count(h) }).rev() {
                let child = unsafe { scan_slot(h, i).read() };
                if child.is_ptr() && unsafe { self.release_ref(child.header())? } {
                    work.push(child);
                }
            }
            unsafe { self.free_box(h) };
        }
        Ok(())
    }

    /// 箱を空ける。`Foreign` なら先に破棄処理を呼ぶ。検査モードは箱を使い回さず、前置きを「解放」にして隔離する。
    unsafe fn free_box(&mut self, h: NonNull<Header>) {
        if unsafe { meta::meta(h) }.kind().has_finalizer() {
            unsafe { foreign::finalize(h) };
        }
        if M::CHECKED {
            unsafe { check::prefix(h) }.set_state(State::Freed, true);
            self.quarantined += unsafe { self.pages.block_bytes(h) };
            self.stats.live -= 1;
        } else {
            unsafe { self.pages.free(h) };
        }
    }

    /// rc が 1 か。reuse とその場の書き換えに使う。共有、不死、張り付いた数は、この比較でみな偽になる。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指す。
    pub unsafe fn is_unique(&self, w: Word) -> Result<bool, HeapError> {
        if !w.is_ptr() {
            return Ok(false);
        }
        let h = w.header();
        unsafe { self.check_live(h)? };
        let rc = unsafe { meta::rc(h) }.load(Ordering::Relaxed);
        unsafe { self.check_owner(h, rc)? };
        Ok(rc == 1)
    }

    /// 手放そうとしている参照が最後のものか (rc が 1 か −1)。フィールドを `dup` せずに受け取ってよいかに使う。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指す。
    pub unsafe fn is_last(&self, w: Word) -> Result<bool, HeapError> {
        if !w.is_ptr() {
            return Ok(false);
        }
        let h = w.header();
        unsafe { self.check_live(h)? };
        let rc = unsafe { meta::rc(h) };
        let last = rc::is_last(rc);
        unsafe { self.check_owner(h, rc.load(Ordering::Relaxed))? };
        Ok(last)
    }

    /// `is_last` が真で、子をすでに処理した物体の箱を空ける。ヘッダの `scan` を読まない。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、呼ぶ側が最後の参照を持ち、子の参照はもう処理してある。
    pub unsafe fn free_block(&mut self, w: Word) -> Result<(), HeapError> {
        let h = w.header();
        if M::CHECKED {
            unsafe { self.check_live(h)? };
            if unsafe { meta::meta(h) }.kind().has_finalizer() {
                return Err(HeapError::FinalizerSkipped);
            }
            let rc = unsafe { meta::rc(h) };
            if !rc::is_last(rc) {
                return Err(HeapError::NotLast);
            }
            unsafe { self.check_owner(h, rc.load(Ordering::Relaxed))? };
            self.stats.rc_decrements += 1;
        }
        unsafe { self.free_box(h) };
        Ok(())
    }

    /// # Safety
    /// `w` はこのヒープの生きている物体を指す。
    pub unsafe fn meta(&self, w: Word) -> Result<Meta, HeapError> {
        unsafe { self.check_live(w.header())? };
        Ok(unsafe { meta::meta(w.header()) })
    }

    /// ペイロードの物理的な i 語目を読む。ヘッダを読まない。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i 語目は走査する語である。
    pub unsafe fn field(&self, w: Word, i: usize) -> Result<Word, HeapError> {
        let h = w.header();
        unsafe {
            self.check_live(h)?;
            self.check_bounds(h, i)?;
            Ok(meta::slot(h, i).read())
        }
    }

    /// ペイロードの物理的な i 語目を書く。参照の数は変えない (所有の受け渡しは呼ぶ側が決める)。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i 語目は走査する語で、ほかのスレッドはその物体を読めない。
    pub unsafe fn set_field(&mut self, w: Word, i: usize, v: Word) -> Result<(), HeapError> {
        let h = w.header();
        unsafe {
            self.check_live(h)?;
            self.check_bounds(h, i)?;
            self.check_owner(h, meta::rc(h).load(Ordering::Relaxed))?;
            meta::slot(h, i).write(v);
        }
        Ok(())
    }

    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i 語目は生の語である。
    pub unsafe fn raw(&self, w: Word, i: usize) -> Result<u64, HeapError> {
        let h = w.header();
        unsafe {
            self.check_live(h)?;
            self.check_bounds(h, i)?;
            Ok(meta::slot(h, i).cast::<u64>().read())
        }
    }

    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、i 語目は生の語で、ほかのスレッドはその物体を読めない。
    pub unsafe fn set_raw(&mut self, w: Word, i: usize, v: u64) -> Result<(), HeapError> {
        let h = w.header();
        unsafe {
            self.check_live(h)?;
            self.check_bounds(h, i)?;
            self.check_owner(h, meta::rc(h).load(Ordering::Relaxed))?;
            meta::slot(h, i).cast::<u64>().write(v);
        }
        Ok(())
    }

    /// 走査する語の数。形を知らない処理が使う。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指す。
    pub unsafe fn scanned_len(&self, w: Word) -> Result<usize, HeapError> {
        unsafe { self.check_live(w.header())? };
        Ok(unsafe { scan_count(w.header()) })
    }

    /// 走査する語の i 番目を読む。`scan` が 255 のときの数の語と、種類の走査の始まりを考える。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指す。
    pub unsafe fn scanned(&self, w: Word, i: usize) -> Result<Word, HeapError> {
        let h = w.header();
        unsafe { self.check_live(h)? };
        if M::CHECKED && i >= unsafe { scan_count(h) } {
            return Err(HeapError::OutOfBounds);
        }
        Ok(unsafe { scan_slot(h, i).read() })
    }

    /// `tobj` の `Int` を読む。即値なら右シフトし、物体なら `BigInt` の中身を読む。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている `BigInt` の物体 (ダミーを含む) を指す。
    pub unsafe fn unbox_int(&self, w: Word) -> Result<i64, HeapError> {
        if w.is_imm() {
            return Ok(w.imm_value());
        }
        let h = w.header();
        unsafe { self.check_live(h)? };
        if M::CHECKED && unsafe { meta::meta(h) }.kind() != Kind::BigInt {
            return Err(HeapError::WrongKind);
        }
        Ok(unsafe { meta::slot(h, 0).cast::<i64>().read() })
    }

    /// `Foreign` の物体の Rust の値の位置。値が `T` でなければ `WrongKind` を返す。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指す。
    pub unsafe fn foreign<T: 'static>(&self, w: Word) -> Result<*mut T, HeapError> {
        let h = w.header();
        unsafe { self.check_live(h)? };
        if unsafe { meta::meta(h) }.kind() != Kind::Foreign {
            return Err(HeapError::WrongKind);
        }
        unsafe { foreign::value::<T>(h) }.ok_or(HeapError::WrongKind)
    }
}
```

`crates/eml_rt/src/heap/walk.rs`:

```rust
//! 生存の列挙、リーク、`Heap` を落とすときの後始末
//! (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「生存の列挙」)。

use std::collections::BTreeMap;
use std::ptr::NonNull;

use super::Heap;
use super::check::{self, State};
use crate::foreign;
use crate::meta::{self, Header, Kind};
use crate::mode::{Checked, Mode};
use crate::sync::Ordering;
use crate::word::Word;

impl<M: Mode> Heap<M> {
    /// 生きている物体 (token の箱、不死の物体、ダミーを含む) を列挙する。S11 の根からの走査が使う。
    pub fn live_objects(&self) -> Vec<Word> {
        let mut live = Vec::new();
        self.pages.for_each_block(|h| {
            if unsafe { self.is_live_block(h) } {
                live.push(Word::from_header(h));
            }
        });
        live
    }

    /// 切り出したブロックが生きている箱か。速いモードでは種類 `Free` の印、検査モードでは前置きの状態で見分ける。
    pub(super) unsafe fn is_live_block(&self, h: NonNull<Header>) -> bool {
        if M::CHECKED {
            unsafe { check::prefix(h) }.state() != State::Freed
        } else {
            unsafe { meta::meta(h) }.kind() != Kind::Free
        }
    }
}

impl Heap<Checked> {
    /// 種類の名前ごとのリーク (spec の「報告する誤り」)。不死の物体は外に出ている参照の数を足し、token の箱は
    /// `Token` で数える。
    pub fn leaks(&self) -> BTreeMap<&'static str, u64> {
        let mut leaks = BTreeMap::new();
        self.pages.for_each_block(|h| {
            let prefix = unsafe { check::prefix(h) };
            let (name, count) = match prefix.state() {
                State::Freed => return,
                State::Token => ("Token", 1),
                State::Alive => {
                    let count = if unsafe { meta::rc(h) }.load(Ordering::Relaxed) == 0 {
                        match prefix.first.load(Ordering::Relaxed) {
                            check::UNCOUNTED => 0,
                            n => u64::from(n),
                        }
                    } else {
                        1
                    };
                    (unsafe { meta::meta(h) }.kind().name(), count)
                }
            };
            if count > 0 {
                *leaks.entry(name).or_default() += count;
            }
        });
        leaks
    }
}

impl<M: Mode> Drop for Heap<M> {
    /// Rust の値が持つメモリを漏らさないように、生きている `Foreign` の破棄処理を呼んでからページを返す。
    fn drop(&mut self) {
        let mut finalizable = Vec::new();
        self.pages.for_each_block(|h| {
            if unsafe { self.is_live_block(h) && meta::meta(h).kind() == Kind::Foreign } {
                finalizable.push(h);
            }
        });
        for h in finalizable {
            unsafe { foreign::finalize(h) };
        }
    }
}
```

`crates/eml_rt/src/lib.rs` を次にする:

```rust
//! VM とネイティブで共有するランタイムの核 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md)。語、ヘッダ、
//! 確保器、RC、`share`、reuse の部品、検査モードを持つ。まだどの crate からも使わない。

mod alloc;
mod error;
mod foreign;
mod heap;
mod meta;
mod mode;
mod rc;
mod sync;
mod word;

pub use error::HeapError;
pub use heap::{Heap, Stats};
pub use meta::{Kind, Meta};
pub use mode::{Checked, Fast, Mode};
pub use word::Word;

/// `Heap` が `Sync` でないことをコンパイルの時点で確かめる。`Sync` なら下の2つの実装のどちらを選ぶかが決まらず、
/// コンパイルが止まる (static_assertions の `assert_not_impl_any!` と同じ方法)。
const _: fn() = || {
    trait AmbiguousIfSync<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> AmbiguousIfSync<()> for T {}
    impl<T: ?Sized + Sync> AmbiguousIfSync<u8> for T {}
    let _ = <Heap<Fast> as AmbiguousIfSync<_>>::some_item;
    let _ = <Heap<Checked> as AmbiguousIfSync<_>>::some_item;
};

#[cfg(test)]
mod tests {
    use super::*;

    fn send_and_sync<T: Send + Sync>() {}

    fn send<T: Send>() {}

    #[test]
    fn words_cross_threads_and_heaps_move_to_one() {
        send_and_sync::<Word>();
        send::<Heap<Fast>>();
        send::<Heap<Checked>>();
    }
}
```

- [ ] **Step 8: テストを流す**

Run: `cargo test -p eml_rt`
Expected: `heap::tests::basics::fast::*` と `heap::tests::basics::checked::*` の 16 ずつ、`the_fast_heap_reuses_a_freed_block`、`heap::tests::checked::*` の 12、前のタスクのテストが通る

- [ ] **Step 9: fmt と clippy**

Run: `cargo fmt && cargo clippy -p eml_rt --all-targets -- -D warnings`
Expected: 警告がない。`dead_code` が残っていたら、その項目が spec の操作に要るかを確かめ、要らないなら消す

- [ ] **Step 10: コミット**

```bash
git add crates/eml_rt
git commit -m "Add the eml_rt heap and its check mode

Heap<M: Mode> allocates, counts references and frees chains with a reused
worklist; the check mode, chosen by type parameter, adds a prefix word per
block with the owner (or the immortal reference count) and the block state,
quarantines freed blocks, and reports leaks by kind.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YLHZowtbHVXEbsTMFYwSMN"
```

---

### Task 6: 共有の経路と `release`

**Files:**
- Create: `crates/eml_rt/src/heap/share.rs`
- Create: `crates/eml_rt/src/heap/release.rs`
- Create: `crates/eml_rt/src/heap/tests/share.rs`
- Modify: `crates/eml_rt/src/heap/mod.rs` (module、フィールド2つ)
- Modify: `crates/eml_rt/src/heap/check.rs` (`check_release`、`layout_check`)
- Modify: `crates/eml_rt/src/heap/walk.rs` (`check_invariants`)
- Modify: `crates/eml_rt/src/heap/tests/mod.rs` (`mod share;`)

**Interfaces:**
- Consumes: Task 5 の `Heap` の内部 (`release_ref`、`drain`、`free_box`、`check_*`、`scan_count`、`scan_slot`、`work`)、`rc::{mark_step, is_last}`
- Produces:
  - `heap::share::{Graph, share_walk}` (crate の中。Kani が使う)。`trait Graph { type Node: Copy + PartialEq; fn rc(&self, Node) -> i32; fn set_rc(&mut self, Node, i32); fn lin(&self, Node) -> bool; fn set_owner(&mut self, Node, u32); fn push_children(&self, Node, &mut Vec<Node>); }`、`fn share_walk<G: Graph>(&mut G, root: G::Node, receiver: u32, check_lin: bool, stack: &mut Vec<G::Node>, visited_lin: &mut Vec<G::Node>) -> Result<(), G::Node>`
  - `unsafe Heap::share(&mut self, w: Word, receiver: u32) -> Result<(), HeapError>`
  - `unsafe Heap::release(&mut self, w: Word, tag: u16, keep: &[bool]) -> Result<(), HeapError>`、crate の中の `unsafe Heap::release_children(&mut self, h, keep: &[bool])`
  - `heap::check::layout_check(meta: Meta, scanned: usize, tag: u16, keep_len: usize) -> Result<(), HeapError>` (Kani が使う)、`unsafe Heap::check_release(&self, h, tag, keep_len)`
  - `Heap<Checked>::check_invariants(&self) -> Result<(), HeapError>`

- [ ] **Step 1: テストを書く**

`crates/eml_rt/src/heap/tests/mod.rs` の `mod checked;` の後に `mod share;` を足す。

`crates/eml_rt/src/heap/tests/share.rs`:

```rust
use std::sync::atomic::Ordering;

use super::{LONG, list, mortal, probe};
use crate::meta;
use crate::sync::Ordering as AtomicOrdering;
use crate::{Checked, Heap, HeapError, Kind, Meta, Mode, Word};

fn rc<M: Mode>(_heap: &Heap<M>, w: Word) -> i32 {
    unsafe { meta::rc(w.header()) }.load(AtomicOrdering::Relaxed)
}

fn sharing_a_list_marks_every_cell<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let l = list(&mut heap, 3);
    unsafe {
        heap.share(l, 0).unwrap();
        let mut cell = l;
        while cell.is_ptr() {
            assert_eq!(rc(&heap, cell), -1);
            cell = heap.field(cell, 1).unwrap();
        }
        assert!(!heap.is_unique(l).unwrap());
        assert!(heap.is_last(l).unwrap());
        heap.decref(l).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn shared_counts_go_through_the_atomic_path<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let x = list(&mut heap, 1);
    unsafe {
        heap.share(x, 0).unwrap();
        heap.dup(x).unwrap();
        assert_eq!(rc(&heap, x), -2);
        assert!(!heap.is_last(x).unwrap());
        heap.decref(x).unwrap();
        assert!(heap.is_last(x).unwrap());
        heap.decref(x).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn share_stops_at_immortal_objects<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let immortal = heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, false));
    let parent = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.acquire_immortal(immortal).unwrap();
        heap.set_field(parent, 0, immortal).unwrap();
        heap.share(parent, 0).unwrap();
        assert_eq!(rc(&heap, immortal), 0);
        assert_eq!(rc(&heap, parent), -1);
        heap.decref(parent).unwrap();
    }
    assert_eq!(mortal(&heap), 1);
}

fn share_walks_a_cycle_once<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = heap.alloc_data(0, 1, 0, false);
    let b = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.set_field(a, 0, b).unwrap();
        // b が a の参照を1つ持つ
        heap.dup(a).unwrap();
        heap.set_field(b, 0, a).unwrap();
        heap.share(a, 0).unwrap();
        assert_eq!((rc(&heap, a), rc(&heap, b)), (-2, -1));
        // 輪を切ってから手放す
        heap.set_field(b, 0, Word::UNIT).unwrap();
        heap.decref(a).unwrap();
        heap.decref(a).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_lin_object_moves_to_the_receiver_and_its_children_are_shared<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let child = list(&mut heap, 1);
    let lin = heap.alloc_data(0, 1, 0, true);
    unsafe {
        heap.set_field(lin, 0, child).unwrap();
        heap.share(lin, 7).unwrap();
        assert_eq!(rc(&heap, lin), 1);
        assert_eq!(rc(&heap, child), -1);
        if M::CHECKED {
            let owner = crate::heap::check::prefix(lin.header()).first.load(AtomicOrdering::Relaxed);
            assert_eq!(owner, 7);
        }
        heap.set_worker(7);
        heap.decref(lin).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn releasing_a_shared_parent_takes_its_lin_child_without_dup<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let (value, drops) = probe();
    let file = heap.alloc_foreign(value, true);
    let rest = list(&mut heap, 1);
    let parent = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.set_field(parent, 0, file).unwrap();
        heap.set_field(parent, 1, rest).unwrap();
        heap.share(parent, 0).unwrap();
        // 検査モードでは、file を dup すると LinDup になる
        heap.release(parent, 0, &[true, false]).unwrap();
        assert_eq!(mortal(&heap), 1);
        heap.decref(file).unwrap();
    }
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    assert_eq!(mortal(&heap), 0);
}

fn a_deep_list_is_shared_without_recursion<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let l = list(&mut heap, LONG);
    unsafe {
        heap.share(l, 0).unwrap();
        heap.decref(l).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn releasing_a_unique_object_hands_over_the_kept_fields<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = list(&mut heap, 1);
    let b = list(&mut heap, 1);
    let p = heap.alloc_data(3, 2, 0, false);
    unsafe {
        heap.set_field(p, 0, a).unwrap();
        heap.set_field(p, 1, b).unwrap();
        heap.release(p, 3, &[true, false]).unwrap();
        assert_eq!(mortal(&heap), 1);
        assert!(heap.is_unique(a).unwrap());
        heap.decref(a).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn releasing_a_shared_object_dups_the_kept_fields<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = list(&mut heap, 1);
    let b = list(&mut heap, 1);
    let p = heap.alloc_data(3, 2, 0, false);
    unsafe {
        heap.set_field(p, 0, a).unwrap();
        heap.set_field(p, 1, b).unwrap();
        heap.dup(p).unwrap();
        heap.release(p, 3, &[true, false]).unwrap();
        assert_eq!(rc(&heap, a), 2);
        assert_eq!(mortal(&heap), 3);
        heap.decref(p).unwrap();
        heap.decref(a).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn the_expanded_release_matches_the_general_one<M: Mode>() {
    // 一意な場合と rc が 2 の場合のそれぞれで、一般の `release` と部品を並べた展開
    // (spec の「核の操作」) を流し、手放した後に生きている箱の数を比べる
    for shared in [false, true] {
        let outcome = |expanded: bool| {
            let mut heap = Heap::<M>::new();
            let a = list(&mut heap, 1);
            let b = list(&mut heap, 1);
            let p = heap.alloc_data(3, 2, 0, false);
            unsafe {
                heap.set_field(p, 0, a).unwrap();
                heap.set_field(p, 1, b).unwrap();
                if shared {
                    heap.dup(p).unwrap();
                }
                if !expanded {
                    heap.release(p, 3, &[true, false]).unwrap();
                } else if heap.is_last(p).unwrap() {
                    heap.decref(b).unwrap();
                    heap.free_block(p).unwrap();
                } else {
                    heap.dup(a).unwrap();
                    heap.decref(p).unwrap();
                }
                let live = mortal(&heap);
                if shared {
                    heap.decref(p).unwrap();
                }
                heap.decref(a).unwrap();
                assert_eq!(mortal(&heap), 0);
                live
            }
        };
        assert_eq!(outcome(false), outcome(true), "shared: {shared}");
    }
}

both_modes!(
    sharing_a_list_marks_every_cell,
    shared_counts_go_through_the_atomic_path,
    share_stops_at_immortal_objects,
    share_walks_a_cycle_once,
    a_lin_object_moves_to_the_receiver_and_its_children_are_shared,
    releasing_a_shared_parent_takes_its_lin_child_without_dup,
    a_deep_list_is_shared_without_recursion,
    releasing_a_unique_object_hands_over_the_kept_fields,
    releasing_a_shared_object_dups_the_kept_fields,
    the_expanded_release_matches_the_general_one,
);

type H = Heap<Checked>;

#[test]
fn the_same_lin_object_reached_twice_is_an_error() {
    let mut heap = H::new();
    let lin = heap.alloc_data(0, 0, 1, true);
    let inner = heap.alloc_data(0, 1, 0, false);
    let outer = heap.alloc_data(0, 2, 0, false);
    unsafe {
        heap.set_field(inner, 0, lin).unwrap();
        heap.set_field(outer, 0, inner).unwrap();
        heap.set_field(outer, 1, lin).unwrap();
        assert_eq!(heap.share(outer, 0), Err(HeapError::LinReachedTwice));
    }
}

#[test]
fn a_shared_object_reaching_a_local_one_breaks_the_invariant() {
    let mut heap = H::new();
    let p = heap.alloc_data(0, 1, 0, false);
    let c = list(&mut heap, 1);
    unsafe {
        heap.share(p, 0).unwrap();
        heap.set_field(p, 0, c).unwrap();
        assert_eq!(heap.check_invariants(), Err(HeapError::SharedReachesLocal));
        heap.set_field(p, 0, Word::UNIT).unwrap();
        heap.decref(c).unwrap();
        heap.decref(p).unwrap();
    }
    assert_eq!(heap.check_invariants(), Ok(()));
}

#[test]
fn an_immortal_object_reaching_a_mortal_one_breaks_the_invariant() {
    let mut heap = H::new();
    let p = heap.alloc_immortal(1, Meta::new(0, 1, Kind::Data, false));
    let c = list(&mut heap, 1);
    unsafe {
        heap.set_field(p, 0, c).unwrap();
        assert_eq!(heap.check_invariants(), Err(HeapError::ImmortalReachesMortal));
        heap.set_field(p, 0, Word::UNIT).unwrap();
        heap.decref(c).unwrap();
    }
}

#[test]
fn a_shared_parent_may_reach_a_local_lin_child() {
    let mut heap = H::new();
    let lin = heap.alloc_data(0, 0, 1, true);
    let p = heap.alloc_data(0, 1, 0, false);
    unsafe {
        heap.set_field(p, 0, lin).unwrap();
        heap.share(p, 0).unwrap();
        assert_eq!(heap.check_invariants(), Ok(()));
        heap.decref(p).unwrap();
    }
}

#[test]
fn a_lin_object_on_the_shared_side_is_an_error() {
    let mut heap = H::new();
    let x = heap.alloc_data(0, 0, 1, false);
    unsafe {
        heap.share(x, 0).unwrap();
        // share は lin の物体に印を付けないので、この形は印付けの誤りでしか起きない
        let m = meta::meta(x.header());
        meta::set_meta(x.header(), m.with_lin(true));
        assert_eq!(heap.decref(x), Err(HeapError::LinShared));
        meta::set_meta(x.header(), m);
        heap.decref(x).unwrap();
    }
}

#[test]
fn using_a_freed_shared_object_is_an_error() {
    let mut heap = H::new();
    let x = list(&mut heap, 1);
    unsafe {
        heap.share(x, 0).unwrap();
        heap.decref(x).unwrap();
        // 最後の共有の decref で rc は 0 になっているが、前置きで解放を見分ける
        assert_eq!(heap.dup(x), Err(HeapError::UseAfterFree));
    }
}

#[test]
fn release_checks_the_layout() {
    let mut heap = H::new();
    let p = heap.alloc_data(3, 2, 0, false);
    unsafe {
        assert_eq!(heap.release(p, 4, &[false, false]), Err(HeapError::WrongLayout));
        assert_eq!(heap.release(p, 3, &[false]), Err(HeapError::WrongLayout));
        heap.release(p, 3, &[false, false]).unwrap();
    }
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_rt heap::tests::share`
Expected: `share`、`release`、`check_invariants` がないので、コンパイルが失敗する

- [ ] **Step 3: `share` を書く**

`crates/eml_rt/src/heap/mod.rs` の module の並びに `mod release;` と `pub(crate) mod share;` を足し、`Heap` に次の2つのフィールドを足す (`work` の後)。`Heap::new` の構造体の式にも `share_stack: Vec::new(), visited_lin: Vec::new(),` を足す。

```rust
    /// `share` のたどりのスタック。使い回す。
    share_stack: Vec<NonNull<Header>>,
    /// 検査モードの `share` が訪れた `lin` の物体。
    visited_lin: Vec<NonNull<Header>>,
```

`crates/eml_rt/src/heap/share.rs`:

```rust
//! `share` (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「`share`」)。たどりは物体への読み書きを
//! `Graph` の裏に置いて書き、ヒープの物体と Kani の有界なグラフのモデルで同じコードを動かす。

use std::marker::PhantomData;
use std::mem;
use std::ptr::NonNull;

use super::{Heap, check, scan_count, scan_slot};
use crate::error::HeapError;
use crate::meta::{self, Header};
use crate::mode::Mode;
use crate::rc;
use crate::sync::Ordering;
use crate::word::Word;

pub(crate) trait Graph {
    type Node: Copy + PartialEq;
    fn rc(&self, node: Self::Node) -> i32;
    fn set_rc(&mut self, node: Self::Node, rc: i32);
    fn lin(&self, node: Self::Node) -> bool;
    fn set_owner(&mut self, node: Self::Node, owner: u32);
    /// 子のうち物体を指すものを `out` に積む。
    fn push_children(&self, node: Self::Node, out: &mut Vec<Self::Node>);
}

/// 根から届く物体に共有の印を付ける。共有と不死の物体でたどりを止める。`lin` の物体には印を付けずに所有者を移し、
/// その子はたどり続ける。`Lin` の物体の中の `Unr` の子は、送り手がほかの所からも参照していることがあるためである。
/// `check_lin` なら、同じ `lin` の物体に2回届いたときにその物体を返す。
pub(crate) fn share_walk<G: Graph>(
    graph: &mut G,
    root: G::Node,
    receiver: u32,
    check_lin: bool,
    stack: &mut Vec<G::Node>,
    visited_lin: &mut Vec<G::Node>,
) -> Result<(), G::Node> {
    stack.clear();
    visited_lin.clear();
    stack.push(root);
    while let Some(node) = stack.pop() {
        if graph.lin(node) {
            if check_lin {
                if visited_lin.contains(&node) {
                    return Err(node);
                }
                visited_lin.push(node);
            }
            graph.set_owner(node, receiver);
            graph.push_children(node, stack);
        } else if let Some(shared) = rc::mark_step(graph.rc(node)) {
            graph.set_rc(node, shared);
            graph.push_children(node, stack);
        }
    }
    Ok(())
}

/// ヒープの物体のグラフ。`share` の安全の条件 (届く物体はどれも生きている) の下でだけ使う。
struct Objects<M>(PhantomData<M>);

impl<M: Mode> Graph for Objects<M> {
    type Node = NonNull<Header>;

    fn rc(&self, h: NonNull<Header>) -> i32 {
        unsafe { meta::rc(h) }.load(Ordering::Relaxed)
    }

    fn set_rc(&mut self, h: NonNull<Header>, rc: i32) {
        unsafe { meta::rc(h) }.store(rc, Ordering::Relaxed);
    }

    fn lin(&self, h: NonNull<Header>) -> bool {
        unsafe { meta::meta(h) }.lin()
    }

    fn set_owner(&mut self, h: NonNull<Header>, owner: u32) {
        if M::CHECKED {
            unsafe { check::prefix(h) }.first.store(owner, Ordering::Relaxed);
        }
    }

    fn push_children(&self, h: NonNull<Header>, out: &mut Vec<NonNull<Header>>) {
        for i in 0..unsafe { scan_count(h) } {
            let child = unsafe { scan_slot(h, i).read() };
            if child.is_ptr() {
                out.push(child.header());
            }
        }
    }
}

impl<M: Mode> Heap<M> {
    /// w から届く物体を、`receiver` に渡すために共有にする。送り手のスレッドが、値を公開する前に呼ぶ。
    ///
    /// # Safety
    /// `w` は即値か、このヒープの生きている物体を指し、そこから届く物体はどれも生きている。
    pub unsafe fn share(&mut self, w: Word, receiver: u32) -> Result<(), HeapError> {
        if !w.is_ptr() {
            return Ok(());
        }
        unsafe { self.check_live(w.header())? };
        let mut stack = mem::take(&mut self.share_stack);
        let mut visited = mem::take(&mut self.visited_lin);
        let result = share_walk(
            &mut Objects::<M>(PhantomData),
            w.header(),
            receiver,
            M::CHECKED,
            &mut stack,
            &mut visited,
        );
        self.share_stack = stack;
        self.visited_lin = visited;
        result.map_err(|_| HeapError::LinReachedTwice)
    }
}
```

- [ ] **Step 4: `release` を書く**

`crates/eml_rt/src/heap/check.rs` に足す。`use crate::meta::{self, HEADER_WORDS, Header, Kind, Meta};` に直し、`use super::scan_count;` を足す。

```rust
/// `release` が名指す形の照らし合わせ。Kani が同じ関数を確かめる。
pub(crate) fn layout_check(meta: Meta, scanned: usize, tag: u16, keep_len: usize) -> Result<(), HeapError> {
    if meta.kind() == Kind::Data && meta.tag() == tag && scanned == keep_len {
        Ok(())
    } else {
        Err(HeapError::WrongLayout)
    }
}

impl<M: Mode> Heap<M> {
    /// `release` と `release_reuse` の検査。
    pub(super) unsafe fn check_release(&self, h: NonNull<Header>, tag: u16, keep_len: usize) -> Result<(), HeapError> {
        if !M::CHECKED {
            return Ok(());
        }
        unsafe { self.check_live(h)? };
        let m = unsafe { meta::meta(h) };
        layout_check(m, unsafe { scan_count(h) }, tag, keep_len)?;
        let rc = unsafe { meta::rc(h) }.load(Ordering::Relaxed);
        if rc < 0 && m.lin() {
            return Err(HeapError::LinShared);
        }
        unsafe { self.check_owner(h, rc) }
    }
}
```

`crates/eml_rt/src/heap/release.rs`:

```rust
//! LIR の `release` (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「核の操作」)。判定は `is_last` で、
//! rc が −1 の物体も最後の参照として、残すフィールドを `dup` せずに受け取る。共有の親から `lin` の子が届く形
//! (spec の「`share`」) で、正しいプログラムの `File` を `dup` しないためである。

use std::mem;
use std::ptr::NonNull;

use super::{Heap, scan_slot};
use crate::error::HeapError;
use crate::meta::{self, Header};
use crate::mode::Mode;
use crate::rc;
use crate::word::Word;

impl<M: Mode> Heap<M> {
    /// `keep` は走査する語ごとに、その子の参照を呼ぶ側に残すかを持つ。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、呼ぶ側がその参照を1つ持っている。
    pub unsafe fn release(&mut self, w: Word, tag: u16, keep: &[bool]) -> Result<(), HeapError> {
        let h = w.header();
        unsafe { self.check_release(h, tag, keep.len())? };
        if rc::is_last(unsafe { meta::rc(h) }) {
            unsafe { self.release_children(h, keep)? };
            if M::CHECKED {
                self.stats.rc_decrements += 1;
            }
            unsafe { self.free_box(h) };
            Ok(())
        } else {
            for (i, &kept) in keep.iter().enumerate() {
                if kept {
                    unsafe { self.dup(scan_slot(h, i).read())? };
                }
            }
            unsafe { self.decref(w) }
        }
    }

    /// 残さない子を手放す。残す子の参照は、呼ぶ側が箱から受け継ぐ。
    pub(super) unsafe fn release_children(&mut self, h: NonNull<Header>, keep: &[bool]) -> Result<(), HeapError> {
        let mut work = mem::take(&mut self.work);
        let result = unsafe { self.release_unkept(h, keep, &mut work) };
        work.clear();
        self.work = work;
        result
    }

    unsafe fn release_unkept(&mut self, h: NonNull<Header>, keep: &[bool], work: &mut Vec<Word>) -> Result<(), HeapError> {
        for i in (0..keep.len()).rev() {
            if keep[i] {
                continue;
            }
            let child = unsafe { scan_slot(h, i).read() };
            if child.is_ptr() && unsafe { self.release_ref(child.header())? } {
                work.push(child);
            }
        }
        unsafe { self.drain(work) }
    }
}
```

`release_ref` と `drain` と `free_box` は `heap/mod.rs` で `unsafe fn` (非公開) なので、同じ module の子の `release.rs` から呼べる。呼べないと言われたら `pub(super)` にする。

- [ ] **Step 5: 不変条件の検査を書く**

`crates/eml_rt/src/heap/walk.rs` の `impl Heap<Checked>` に足す。`use super::{scan_count, scan_slot};` と `use crate::error::HeapError;` を足す。

```rust
    /// 生存の列挙で見つかる不変条件の違反 (spec の「報告する誤り」)。共有の物体から届いてよいのは共有か不死の物体か
    /// `lin` ビットの立った局所の物体で、不死の物体から届いてよいのは不死の物体だけである。
    pub fn check_invariants(&self) -> Result<(), HeapError> {
        let mut result = Ok(());
        self.pages.for_each_block(|h| {
            if result.is_err() || unsafe { check::prefix(h) }.state() != State::Alive {
                return;
            }
            let rc = unsafe { meta::rc(h) }.load(Ordering::Relaxed);
            if rc > 0 {
                return;
            }
            for i in 0..unsafe { scan_count(h) } {
                let child = unsafe { scan_slot(h, i).read() };
                if !child.is_ptr() {
                    continue;
                }
                let c = child.header();
                if unsafe { check::prefix(c) }.state() != State::Alive {
                    result = Err(HeapError::UseAfterFree);
                    return;
                }
                let child_rc = unsafe { meta::rc(c) }.load(Ordering::Relaxed);
                if rc == 0 && child_rc != 0 {
                    result = Err(HeapError::ImmortalReachesMortal);
                    return;
                }
                if rc < 0 && child_rc > 0 && !unsafe { meta::meta(c) }.lin() {
                    result = Err(HeapError::SharedReachesLocal);
                    return;
                }
            }
        });
        result
    }
```

- [ ] **Step 6: テストを流す**

Run: `cargo test -p eml_rt`
Expected: `heap::tests::share::fast::*` と `heap::tests::share::checked::*` の 10 ずつ、`heap::tests::share` の検査モードだけの 7 つ、前のタスクのテストが通る

- [ ] **Step 7: fmt、clippy、コミット**

Run: `cargo fmt && cargo clippy -p eml_rt --all-targets -- -D warnings`

```bash
git add crates/eml_rt
git commit -m "Add share, shared RC, and release to eml_rt

share marks what a value reaches as shared and moves lin objects to the
receiver while still walking their children; release decides with
is_last, so a shared parent at rc -1 hands over its lin child without a
dup. The check mode verifies the shared and immortal reachability rules.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YLHZowtbHVXEbsTMFYwSMN"
```

---

### Task 7: reuse の token

**Files:**
- Create: `crates/eml_rt/src/heap/reuse.rs`
- Create: `crates/eml_rt/src/heap/tests/reuse.rs`
- Modify: `crates/eml_rt/src/heap/mod.rs` (module、`tokens` のフィールド、`pub use`)
- Modify: `crates/eml_rt/src/heap/check.rs` (`reuse_in_place_check`)
- Modify: `crates/eml_rt/src/heap/tests/mod.rs` (`mod reuse;`)
- Modify: `crates/eml_rt/src/lib.rs` (`Token`)

**Interfaces:**
- Consumes: Task 5 と Task 6 の `Heap` の内部 (`take_token` から使う `check_live`、`check_owner`、`release_children`、`check_release`、`free_box`、`object_words`)
- Produces:
  - `Token` (`#[must_use]`、`EMPTY`、`is_empty`、`into_word`、`unsafe from_word`)
  - `unsafe Heap::{into_token(w) -> Result<Token>, con_at(token, payload_words, meta) -> Result<(Word, bool)>, reuse_in_place(token, meta) -> Result<Word>, drop_token(token) -> Result<()>, release_reuse(w, tag, keep) -> Result<Token>}`
  - `heap::check::reuse_in_place_check(boxed: Meta, new: Meta) -> Result<(), HeapError>` (Kani が使う)

- [ ] **Step 1: テストを書く**

`crates/eml_rt/src/heap/tests/mod.rs` の `mod share;` の後に `mod reuse;` を足す。

`crates/eml_rt/src/heap/tests/reuse.rs`:

```rust
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use super::{CONS, cons, list, mortal, probe};
use crate::{Checked, Heap, HeapError, Kind, Meta, Mode, Token, Word};

const CONS_META: Meta = Meta::new(CONS, 2, Kind::Data, false);
const PAIR_META: Meta = Meta::new(0, 2, Kind::Data, false);

fn a_unique_cell_is_reused_in_place_without_writing_fields<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let a = list(&mut heap, 1);
    let b = list(&mut heap, 1);
    let p = cons(&mut heap, a, b);
    unsafe {
        let token = heap.release_reuse(p, CONS, &[true, true]).unwrap();
        assert!(!token.is_empty());
        let q = heap.reuse_in_place(token, CONS_META).unwrap();
        assert_eq!(q, p);
        assert_eq!((heap.field(q, 0), heap.field(q, 1)), (Ok(a), Ok(b)));
        assert_eq!(mortal(&heap), 3);
        heap.decref(q).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_box_moves_to_another_type_of_the_same_size<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let cell = cons(&mut heap, Word::imm(1), Word::imm(0));
    unsafe {
        let token = heap.release_reuse(cell, CONS, &[false, false]).unwrap();
        let (pair, reused) = heap.con_at(token, 2, PAIR_META).unwrap();
        assert!(reused);
        assert_eq!(pair, cell);
        assert_eq!(heap.meta(pair), Ok(PAIR_META));
        heap.set_field(pair, 0, Word::imm(2)).unwrap();
        heap.set_field(pair, 1, Word::imm(3)).unwrap();
        heap.decref(pair).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_shared_object_is_never_a_token<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let p = cons(&mut heap, Word::imm(1), Word::imm(0));
    unsafe {
        heap.dup(p).unwrap();
        let token = heap.release_reuse(p, CONS, &[false, false]).unwrap();
        assert!(token.is_empty());
        let (q, reused) = heap.con_at(token, 2, CONS_META).unwrap();
        assert!(!reused);
        assert_ne!(q, p);
        heap.set_field(q, 0, Word::UNIT).unwrap();
        heap.set_field(q, 1, Word::UNIT).unwrap();
        heap.decref(p).unwrap();
        heap.decref(q).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_shared_marked_object_is_never_a_token<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let p = cons(&mut heap, Word::imm(1), Word::imm(0));
    unsafe {
        heap.share(p, 0).unwrap();
        // rc −1 は最後の参照だが共有の印を持つので、箱を使い回さずに空ける
        assert!(heap.release_reuse(p, CONS, &[false, false]).unwrap().is_empty());
    }
    assert_eq!(mortal(&heap), 0);
}

fn drop_token_frees_the_box<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let p = cons(&mut heap, Word::imm(1), Word::imm(0));
    unsafe {
        let token = heap.into_token(p).unwrap();
        heap.drop_token(token).unwrap();
    }
    assert_eq!(mortal(&heap), 0);
}

fn a_heap_dropped_with_an_outstanding_token_and_large_objects_is_clean<M: Mode>() {
    let mut heap = Heap::<M>::new();
    let p = cons(&mut heap, Word::imm(1), Word::imm(0));
    let _token = unsafe { heap.into_token(p) }.unwrap();
    heap.alloc_data(0, 0, 500, false);
    heap.alloc_immortal(1, Meta::new(0, 0, Kind::Data, false));
    let (value, drops) = probe();
    heap.alloc_foreign(value, true);
    drop(heap);
    assert_eq!(drops.load(Ordering::Relaxed), 1);
}

both_modes!(
    a_unique_cell_is_reused_in_place_without_writing_fields,
    a_box_moves_to_another_type_of_the_same_size,
    a_shared_object_is_never_a_token,
    a_shared_marked_object_is_never_a_token,
    drop_token_frees_the_box,
    a_heap_dropped_with_an_outstanding_token_and_large_objects_is_clean,
);

type H = Heap<Checked>;

#[test]
fn a_token_cannot_be_used_twice_even_after_its_box_is_a_token_again() {
    let mut heap = H::new();
    let p = cons(&mut heap, Word::imm(1), Word::imm(0));
    unsafe {
        let first = heap.into_token(p).unwrap().into_word();
        let (q, _) = heap.con_at(Token::from_word(first), 2, CONS_META).unwrap();
        // 同じ箱がまた token になっても、古い token は断る (spec の「検査モードの token」)
        let second = heap.into_token(q).unwrap();
        assert_eq!(heap.con_at(Token::from_word(first), 2, CONS_META), Err(HeapError::StaleToken));
        heap.drop_token(second).unwrap();
    }
    assert!(heap.leaks().is_empty());
}

#[test]
fn only_unique_data_becomes_a_token() {
    let mut heap = H::new();
    let f = heap.alloc_foreign(0u64, false);
    let p = list(&mut heap, 1);
    unsafe {
        assert_eq!(heap.into_token(f), Err(HeapError::NotTokenable));
        heap.dup(p).unwrap();
        assert_eq!(heap.into_token(p), Err(HeapError::NotUnique));
        heap.decref(p).unwrap();
        heap.decref(p).unwrap();
        heap.decref(f).unwrap();
    }
}

#[test]
fn a_token_of_another_class_is_an_error() {
    let mut heap = H::new();
    let p = cons(&mut heap, Word::UNIT, Word::UNIT);
    unsafe {
        let token = heap.into_token(p).unwrap();
        assert_eq!(heap.con_at(token, 10, CONS_META), Err(HeapError::WrongClass));
    }
}

#[test]
fn reuse_in_place_rejects_a_kept_lin_bit_and_another_shape() {
    let mut heap = H::new();
    let lin = heap.alloc_data(CONS, 2, 0, true);
    let plain = cons(&mut heap, Word::UNIT, Word::UNIT);
    let other = cons(&mut heap, Word::UNIT, Word::UNIT);
    unsafe {
        let token = heap.into_token(lin).unwrap();
        assert_eq!(heap.reuse_in_place(token, CONS_META), Err(HeapError::LinBitKept));
        let token = heap.into_token(plain).unwrap();
        assert_eq!(heap.reuse_in_place(token, PAIR_META), Err(HeapError::WrongMeta));
        // ビットが欠けるのは安全の側なので通す
        let token = heap.into_token(other).unwrap();
        let reused = heap.reuse_in_place(token, CONS_META.with_lin(true)).unwrap();
        heap.decref(reused).unwrap();
    }
}

#[test]
fn an_unused_token_is_a_leak() {
    let mut heap = H::new();
    let p = cons(&mut heap, Word::UNIT, Word::UNIT);
    unsafe {
        let token = heap.into_token(p).unwrap();
        assert_eq!(heap.leaks(), BTreeMap::from([("Token", 1)]));
        heap.drop_token(token).unwrap();
    }
    assert!(heap.leaks().is_empty());
}

#[test]
fn another_worker_cannot_take_a_token() {
    let mut heap = H::new();
    let p = cons(&mut heap, Word::UNIT, Word::UNIT);
    heap.set_worker(1);
    assert_eq!(unsafe { heap.into_token(p) }, Err(HeapError::WrongOwner { owner: 0, worker: 1 }));
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_rt heap::tests::reuse`
Expected: `Token` と reuse の操作がないので、コンパイルが失敗する

- [ ] **Step 3: token を書く**

`crates/eml_rt/src/heap/check.rs` に足す:

```rust
/// `reuse_in_place` の照らし合わせ (spec の「`lin` ビット」)。`lin` ビットは、箱が持ち新しい `meta` が持たないときだけ
/// 誤りにする。ビットが欠けるのは安全の側だからである。Kani が同じ関数を確かめる。
pub(crate) fn reuse_in_place_check(boxed: Meta, new: Meta) -> Result<(), HeapError> {
    if !boxed.same_shape(new) {
        return Err(HeapError::WrongMeta);
    }
    if boxed.lin() && !new.lin() {
        return Err(HeapError::LinBitKept);
    }
    Ok(())
}
```

`crates/eml_rt/src/heap/mod.rs` に `mod reuse;` と `pub use reuse::Token;` を足し、`Heap` に次のフィールドを足す (`visited_lin` の後)。`Heap::new` の構造体の式にも `tokens: Vec::new(),` を足す。

```rust
    /// 検査モードの token の表。項目は箱と、`into_token` のときの世代の番号である。
    tokens: Vec<Option<(NonNull<Header>, u32)>>,
```

`crates/eml_rt/src/lib.rs` の `pub use heap::{Heap, Stats};` を `pub use heap::{Heap, Stats, Token};` にする。

`crates/eml_rt/src/heap/reuse.rs`:

```rust
//! reuse の部品 (docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md の「reuse」と「検査モードの token」)。
//! token の箱は中身を消さない。一意の道で、同じ位置の同じフィールドを書かずに済ませる特殊化が、古いフィールドに頼る
//! ためである。

use std::ptr::NonNull;

use super::check::{self, State};
use super::{Heap, object_words};
use crate::error::HeapError;
use crate::meta::{self, Header, Meta};
use crate::mode::Mode;
use crate::sync::Ordering;
use crate::word::Word;

/// reuse の token。どちらのモードでも1語で、VM のレジスタと後のネイティブの値の大きさをモードで変えない。速いモードでは
/// 箱を指す語、検査モードでは token の表の添字の即値、空の token は `Word::EMPTY` である。
#[must_use]
#[derive(Debug, PartialEq, Eq)]
pub struct Token(Word);

impl Token {
    pub const EMPTY: Token = Token(Word::EMPTY);

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// VM のレジスタに置くための語。
    pub fn into_word(self) -> Word {
        self.0
    }

    /// # Safety
    /// `w` は同じヒープの `into_word` が返した語である。
    pub unsafe fn from_word(w: Word) -> Token {
        Token(w)
    }
}

impl<M: Mode> Heap<M> {
    /// 一意な物体の箱を、フィールドに触れずに token にする。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、呼ぶ側がその唯一の参照を持ち、子の参照はもう処理してある。
    pub unsafe fn into_token(&mut self, w: Word) -> Result<Token, HeapError> {
        if !M::CHECKED {
            return Ok(Token(w));
        }
        let h = w.header();
        unsafe { self.check_live(h)? };
        let rc = unsafe { meta::rc(h) }.load(Ordering::Relaxed);
        unsafe { self.check_owner(h, rc)? };
        if !unsafe { meta::meta(h) }.kind().tokenable() {
            return Err(HeapError::NotTokenable);
        }
        if rc != 1 {
            return Err(HeapError::NotUnique);
        }
        let prefix = unsafe { check::prefix(h) };
        prefix.set_state(State::Token, true);
        // 添字は使い回さない。使った項目を空のまま残して、古い token を確実に断るためである
        self.tokens.push(Some((h, prefix.generation())));
        Ok(Token(Word::imm((self.tokens.len() - 1) as i64)))
    }

    /// token の箱を取り出す。検査モードでは、表の項目、箱の状態、世代の番号を確かめ、項目を空ける。
    unsafe fn take_token(&mut self, token: Token) -> Result<Option<NonNull<Header>>, HeapError> {
        if token.is_empty() {
            return Ok(None);
        }
        if !M::CHECKED {
            return Ok(Some(token.0.header()));
        }
        let index = usize::try_from(token.0.imm_value()).map_err(|_| HeapError::StaleToken)?;
        let (h, generation) = self
            .tokens
            .get_mut(index)
            .and_then(Option::take)
            .ok_or(HeapError::StaleToken)?;
        let prefix = unsafe { check::prefix(h) };
        if prefix.state() != State::Token || prefix.generation() != generation {
            return Err(HeapError::StaleToken);
        }
        Ok(Some(h))
    }

    /// token が空でなければ、その箱に `meta` を書いて返す。空なら確保する。箱を使ったかも返す。フィールドは呼ぶ側が書く。
    ///
    /// # Safety
    /// `token` はこのヒープの token で、まだ使っていない。
    pub unsafe fn con_at(&mut self, token: Token, payload_words: usize, meta: Meta) -> Result<(Word, bool), HeapError> {
        let Some(h) = (unsafe { self.take_token(token)? }) else {
            return Ok((self.alloc(payload_words, meta), false));
        };
        if M::CHECKED {
            if !unsafe { self.pages.fits(h, object_words(payload_words)) } {
                return Err(HeapError::WrongClass);
            }
            unsafe { check::prefix(h) }.set_state(State::Alive, false);
        }
        unsafe { meta::set_meta(h, meta) };
        Ok((Word::from_header(h), true))
    }

    /// 同じコンストラクタの reuse。ヘッダに何も書かずに箱を返す。`meta` は検査モードが照らすためだけに渡す。
    ///
    /// # Safety
    /// `token` はこのヒープの空でない token で、まだ使っていない。
    pub unsafe fn reuse_in_place(&mut self, token: Token, meta: Meta) -> Result<Word, HeapError> {
        let h = unsafe { self.take_token(token)? }.ok_or(HeapError::StaleToken)?;
        if M::CHECKED {
            check::reuse_in_place_check(unsafe { meta::meta(h) }, meta)?;
            unsafe { check::prefix(h) }.set_state(State::Alive, false);
        }
        Ok(Word::from_header(h))
    }

    /// 使わなかった token の箱を空ける。
    ///
    /// # Safety
    /// `token` はこのヒープの token で、まだ使っていない。
    pub unsafe fn drop_token(&mut self, token: Token) -> Result<(), HeapError> {
        if let Some(h) = unsafe { self.take_token(token)? } {
            unsafe { self.free_box(h) };
        }
        Ok(())
    }

    /// 一意なら、残さない子を手放して箱を token にする。そうでなければ `release` と同じにして、空の token を返す。
    ///
    /// # Safety
    /// `w` はこのヒープの生きている物体を指し、呼ぶ側がその参照を1つ持っている。
    pub unsafe fn release_reuse(&mut self, w: Word, tag: u16, keep: &[bool]) -> Result<Token, HeapError> {
        if unsafe { self.is_unique(w)? } {
            let h = w.header();
            unsafe {
                self.check_release(h, tag, keep.len())?;
                self.release_children(h, keep)?;
                self.into_token(w)
            }
        } else {
            unsafe { self.release(w, tag, keep)? };
            Ok(Token::EMPTY)
        }
    }
}
```

`free_box`、`check_release`、`release_children` が `reuse.rs` から見えないと言われたら、`pub(super)` にする。

- [ ] **Step 4: テストを流す**

Run: `cargo test -p eml_rt`
Expected: `heap::tests::reuse::fast::*` と `heap::tests::reuse::checked::*` の 6 ずつ、検査モードだけの 6 つ、前のタスクのテストが通る

- [ ] **Step 5: fmt、clippy、コミット**

Run: `cargo fmt && cargo clippy -p eml_rt --all-targets -- -D warnings`

```bash
git add crates/eml_rt
git commit -m "Add reuse tokens to eml_rt

into_token keeps a unique box with its fields intact so the VM can skip
rewriting unchanged fields; con_at rewrites only meta, reuse_in_place
writes nothing. The check mode keeps a never-reused token table with the
block generation, so a token used twice is caught even after its box
became a token again.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YLHZowtbHVXEbsTMFYwSMN"
```

---

### Task 8: Verus の土台と `share` の証明

**Files:**
- Delete: `crates/eml_rt/src/proofs.rs` (Kani のハーネス。まだコミットしていない)
- Modify: `crates/eml_rt/src/lib.rs` (`#[cfg(kani)] mod proofs;` を消し、`#[cfg(test)] mod verus_copies;` を足す)
- Modify: `crates/eml_rt/Cargo.toml` (`check-cfg` から `cfg(kani)` を消す)
- Modify: `crates/eml_rt/src/heap/share.rs` (`share_walk` を Verus が扱える形にする、module のコメント)
- Create: `crates/eml_rt/src/verus_copies.rs` (写しの照合のテスト)
- Create: `crates/eml_rt/verus/lib.rs`、`crates/eml_rt/verus/rc.rs`、`crates/eml_rt/verus/share.rs`、`crates/eml_rt/verus/check.sh`

**Interfaces:**
- Consumes: `heap::share::{Graph, ShareError, share_walk}` (Task 6)、`rc::{DROP_LIMIT, mark_step}` (Task 3)
- Produces: `crates/eml_rt/verus/` の Verus の crate (`check.sh` で流す)、`verus_copies.rs` の `COPIES` の表 (Task 9 が行を足す)

spec の「Verus」の土台を作り、証明の4 (`share`) を通す。Verus の試作 (`share_walk` の写しと証明。dispatch でパスを渡す) が、任意のグラフについてすでに通っている。それを `crates/eml_rt/verus/share.rs` に移し、本体との照合のテストを足す。

Verus は Nix の外で動かす。導入は次のとおりで、controller の環境ではもう入っている。

```sh
mkdir -p ~/.verus && cd ~/.verus
gh release download release/0.2026.10.04.426d8b0 -R verus-lang/verus -p 'verus-0.2026.10.04.426d8b0-arm64-macos.zip'
unzip -q verus-0.2026.10.04.426d8b0-arm64-macos.zip   # ~/.verus/verus-arm64-macos/verus ができる
rustup install 1.98.1-aarch64-apple-darwin           # Verus が求める toolchain
```

- [ ] **Step 1: Kani のハーネスを消す**

`crates/eml_rt/src/proofs.rs` を消し、`crates/eml_rt/src/lib.rs` の `#[cfg(kani)]` と `mod proofs;` の2行を消す。`crates/eml_rt/Cargo.toml` の `check-cfg` を `["cfg(loom)"]` にする (`cfg(loom)` は Task 10 で消す)。

- [ ] **Step 2: 照合のテストを書く**

`crates/eml_rt/src/lib.rs` の `mod word;` の後に足す:

```rust
#[cfg(test)]
mod verus_copies;
```

`crates/eml_rt/src/verus_copies.rs`:

```rust
//! Verus の写しが本体と同じコードであることを確かめる (spec の「Verus」)。両方からコメントと属性と空白を除き、写しから
//! はさらに Verus だけの書き方を除いて比べる。

/// 比べる項目。名前は、ファイルの一番外の `fn`、`struct`、`enum`、`trait`、`const` か、`型::名前` で `impl 型` の中の
/// `fn` か `const` を指す。表に載らない型は、写しの側で証明に要る部分だけを持てばよい。
const COPIES: &[(&str, &str, &[&str])] = &[
    (
        include_str!("rc.rs"),
        include_str!("../verus/rc.rs"),
        &["DROP_LIMIT", "mark_step"],
    ),
    (
        include_str!("heap/share.rs"),
        include_str!("../verus/share.rs"),
        &["Graph", "ShareError", "share_walk"],
    ),
];

/// 仕様の節の始まりの語。節は、その後の深さ 0 の `{` か `;` の前で終わる。そのため写しの節には `{` を書かない。
const CLAUSES: &[&str] = &[
    "requires",
    "ensures",
    "recommends",
    "invariant",
    "invariant_except_break",
    "decreases",
    "returns",
    "opens_invariants",
];

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `at` から始まる語が `word` で、前後が識別子の続きでないか。
fn word_at(s: &[char], at: usize, word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    s.len() >= at + w.len()
        && s[at..at + w.len()] == w[..]
        && (at == 0 || !is_ident(s[at - 1]))
        && s.get(at + w.len()).is_none_or(|&c| !is_ident(c))
}

fn skip_space(s: &[char], mut at: usize) -> usize {
    while at < s.len() && s[at].is_whitespace() {
        at += 1;
    }
    at
}

/// `s[open]` の括弧に対応する閉じ括弧の位置。3種類の括弧をまとめて数える。
fn matching(s: &[char], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, &c) in s.iter().enumerate().skip(open) {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced brackets");
}

/// `from` から、深さ 0 の `stops` のどれかの位置 (なければ末尾)。
fn find_at_depth_0(s: &[char], from: usize, stops: &[char]) -> usize {
    let mut i = from;
    while i < s.len() {
        match s[i] {
            c if stops.contains(&c) => return i,
            '(' | '[' => i = matching(s, i) + 1,
            _ => i += 1,
        }
    }
    s.len()
}

/// 行のコメントと属性を除く。比べるコードは、文字列のリテラルの中に `//` も `#[` も書かない。
fn strip_comments_and_attributes(text: &str) -> Vec<char> {
    let mut s: Vec<char> = Vec::new();
    for line in text.lines() {
        let code = line.find("//").map_or(line, |i| &line[..i]);
        s.extend(code.chars());
        s.push('\n');
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == '#' {
            let mut j = skip_space(&s, i + 1);
            if s.get(j) == Some(&'!') {
                j = skip_space(&s, j + 1);
            }
            if s.get(j) == Some(&'[') {
                i = matching(&s, j) + 1;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

/// 項目の終わり: 深さ 0 の `;` か、最初の深さ 0 の `{` に対応する `}`。
fn item_end(s: &[char], from: usize) -> usize {
    let stop = find_at_depth_0(s, from, &['{', ';']);
    if s.get(stop) == Some(&'{') {
        matching(s, stop) + 1
    } else {
        stop + 1
    }
}

/// `s` の中で、`fn` などに続く名前 `name` の項目を、その行の始まりから取り出す。
fn find_item(s: &[char], name: &str) -> Option<(usize, usize)> {
    const KEYWORDS: &[&str] = &["fn", "struct", "enum", "trait", "const"];
    let mut depth = 0usize;
    for i in 0..s.len() {
        match s[i] {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth != 0 {
            continue;
        }
        for keyword in KEYWORDS {
            if word_at(s, i, keyword) {
                let at = skip_space(s, i + keyword.len());
                if word_at(s, at, name) {
                    let start = s[..i].iter().rposition(|&c| c == '\n').map_or(0, |p| p + 1);
                    return Some((start, item_end(s, i)));
                }
            }
        }
    }
    None
}

/// 写しの項目は `verus! { }` の中にあるので、その中身を一番外として扱う。
fn unwrap_verus(s: Vec<char>) -> Vec<char> {
    let Some(at) = (0..s.len()).find(|&i| word_at(&s, i, "verus") && s.get(skip_space(&s, i + 5)) == Some(&'!'))
    else {
        return s;
    };
    let open = find_at_depth_0(&s, at, &['{']);
    s[open + 1..matching(&s, open)].to_vec()
}

fn extract(text: &str, path: &str) -> Vec<char> {
    let s = unwrap_verus(strip_comments_and_attributes(text));
    let (scope, name) = match path.split_once("::") {
        Some((ty, name)) => (Some(ty), name),
        None => (None, path),
    };
    let body: &[char] = match scope {
        None => &s,
        Some(ty) => {
            let open = (0..s.len())
                .find(|&i| {
                    word_at(&s, i, "impl") && {
                        let at = skip_space(&s, i + 4);
                        word_at(&s, at, ty) && s.get(skip_space(&s, at + ty.len())) == Some(&'{')
                    }
                })
                .unwrap_or_else(|| panic!("no `impl {ty}`"));
            let open = find_at_depth_0(&s, open, &['{']);
            &s[open + 1..matching(&s, open)]
        }
    };
    let (start, end) = find_item(body, name).unwrap_or_else(|| panic!("no item `{path}`"));
    body[start..end].to_vec()
}

/// 写しから、仕様の節、`proof` の塊、`let ghost` の文、`spec fn` の宣言、戻り値の名前を除く。
fn strip_verus(s: &[char]) -> Vec<char> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if word_at(s, i, "proof") && s.get(skip_space(s, i + 5)) == Some(&'{') {
            i = matching(s, skip_space(s, i + 5)) + 1;
            continue;
        }
        if word_at(s, i, "let") && word_at(s, skip_space(s, i + 3), "ghost") {
            i = find_at_depth_0(s, i, &[';']) + 1;
            continue;
        }
        if word_at(s, i, "spec") && word_at(s, skip_space(s, i + 4), "fn") {
            // `pub open spec fn` の修飾も、行の始まりから除く
            while out.last().is_some_and(|&c: &char| c != '\n') {
                out.pop();
            }
            i = item_end(s, i);
            continue;
        }
        if let Some(clause) = CLAUSES.iter().find(|clause| word_at(s, i, clause)) {
            i = find_at_depth_0(s, i + clause.len(), &['{', ';']);
            continue;
        }
        if s[i] == '-' && s.get(i + 1) == Some(&'>') {
            let open = skip_space(s, i + 2);
            if s.get(open) == Some(&'(') {
                let mut j = skip_space(s, open + 1);
                let ident = j;
                while j < s.len() && is_ident(s[j]) {
                    j += 1;
                }
                let colon = skip_space(s, j);
                if j > ident && s.get(colon) == Some(&':') && s.get(colon + 1) != Some(&':') {
                    let close = matching(s, open);
                    out.extend(['-', '>', ' ']);
                    out.extend(&s[colon + 1..close]);
                    i = close + 1;
                    continue;
                }
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

fn squeeze(s: &[char]) -> String {
    s.iter().filter(|c| !c.is_whitespace()).collect()
}

/// 写しの比べる形。証明だけを置いた `else` の枝は、証明を除くと空になるので、それも除く。
fn normalize_copy(copy: &str, item: &str) -> String {
    squeeze(&strip_verus(&extract(copy, item))).replace("else{}", "")
}

#[test]
fn the_verus_copies_match_the_code() {
    for (code, copy, items) in COPIES {
        for item in *items {
            let expected = squeeze(&extract(code, item));
            let actual = normalize_copy(copy, item);
            assert_eq!(actual, expected, "the Verus copy of `{item}` differs from the code");
        }
    }
}

#[test]
fn verus_only_syntax_is_removed() {
    let copy = "fn f(x: u8) -> (r: Result<(), E>)\n    requires\n        x > 0,\n    ensures\n        r is Ok,\n{\n    let ghost g = x;\n    proof {\n        assert(g == x);\n    }\n    loop\n        invariant\n            g == x,\n        decreases 0nat,\n    {\n        if x > 1 {\n            break;\n        } else {\n            proof {\n                assert(x == 1);\n            }\n        }\n    }\n    Ok(())\n}\n";
    let code = "fn f(x: u8) -> Result<(), E> {\n    loop {\n        if x > 1 {\n            break;\n        }\n    }\n    Ok(())\n}\n";
    assert_eq!(normalize_copy(copy, "f"), squeeze(&extract(code, "f")));
}

#[test]
fn trait_specs_and_spec_fns_are_removed() {
    let copy = "pub trait T: Sized {\n    type N: Copy;\n\n    spec fn view(&self) -> Seq<u8>;\n\n    fn get(&self, i: usize) -> (r: u8)\n        requires\n            i < self.view().len(),\n    ;\n}\n";
    let code = "/// 説明\npub trait T: Sized {\n    type N: Copy;\n    fn get(&self, i: usize) -> u8;\n}\n";
    assert_eq!(normalize_copy(copy, "T"), squeeze(&extract(code, "T")));
}

#[test]
fn items_in_an_impl_are_found_by_type_and_name() {
    let code = "#[derive(Clone)]\npub struct M(u32);\n\nimpl M {\n    const L: u32 = 1 << 31; // 説明\n    pub const fn new(x: u32) -> M {\n        M(x)\n    }\n}\n\nfn new() {}\n";
    assert_eq!(squeeze(&extract(code, "M::L")), "constL:u32=1<<31;");
    assert_eq!(squeeze(&extract(code, "M::new")), "pubconstfnnew(x:u32)->M{M(x)}");
    assert_eq!(squeeze(&extract(code, "M")), "pubstructM(u32);");
    assert_eq!(squeeze(&extract(code, "new")), "fnnew(){}");
}

#[test]
#[should_panic(expected = "differs")]
fn a_changed_body_is_reported() {
    let copy = "fn f() -> (r: u8)\n    ensures\n        r == 1,\n{\n    1\n}\n";
    let code = "fn f() -> u8 {\n    2\n}\n";
    let expected = squeeze(&extract(code, "f"));
    let actual = normalize_copy(copy, "f");
    assert_eq!(actual, expected, "the Verus copy of `f` differs from the code");
}
```

Run: `cargo test -p eml_rt verus_copies`
Expected: 組み立たない (`../verus/share.rs` がない)

- [ ] **Step 3: 本体の `share_walk` を Verus が扱える形にする**

Verus の証明は、取り出す前の積み場に名前を付ける (`let ghost s = stack@;`)。`while let` ではそれができないので、本体も `loop` と `match` で書く。`crates/eml_rt/src/heap/share.rs` の `while let Some(node) = stack.pop() {` を次にする。ループの中身は変えない:

```rust
    // Verus の写しと同じ形にする。写しは、取り出す前の積み場に名前を付けて証明する
    #[allow(clippy::while_let_loop)]
    loop {
        let node = match stack.pop() {
            Some(node) => node,
            None => break,
        };
```

module の先頭のコメントの2文目を「たどりは物体への読み書きを `Graph` の裏に置いて書き、Verus がたどりを trait のメソッドの約束の上で証明する。証明するのは `crates/eml_rt/verus/share.rs` の写しで、写しが本体と同じことは `verus_copies.rs` のテストが確かめる。」にする。

- [ ] **Step 4: 写しを置く**

`crates/eml_rt/verus/lib.rs`:

```rust
//! `eml_rt` の純粋な関数と `share` のたどりの、Verus の写し (spec の「Verus」)。Cargo のビルドには入らず、`check.sh`
//! で流す。写しの関数は本体と同じコードに仕様と証明を足したもので、
//! 本体と同じことは `crates/eml_rt/src/verus_copies.rs` のテストが確かめる。

mod rc;
mod share;
```

`crates/eml_rt/verus/rc.rs` に、本体の `rc.rs` の `DROP_LIMIT` と `mark_step` を本体と同じ文字で写し、`verus!` の中に置く。`mark_step` には、試作と同じ `ensures` (rc が 0 以下なら `None`、正なら `Some(mark_spec(rc))`) を付ける。`mark_spec` は rc.rs の `spec fn` にする。`DROP_LIMIT` を `i32::MIN + (1 << 17)` のまま Verus が受け付けなければ、報告して、本体と写しの両方を Verus が受け付ける同じ式にする。

`crates/eml_rt/verus/share.rs` は、試作を次のように直して置く。

- 試作の `mark_step`、`mark_spec`、`DROP_LIMIT` は rc.rs に移したので消し、`use crate::rc::{self, mark_spec};` で使う。本体と同じく `rc::mark_step(rc)` と書く
- `Graph`、`ShareError`、`share_walk` の可視性を本体と同じ `pub(crate)` にする。`trait Graph` に `Sized` を付けない
- `ShareError` の変体の並びと名前、`share_walk` の引数の名前と並び、ループの中身を本体と同じにする。照合のテストが、コメント、属性、空白、Verus だけの書き方の違いだけを許す
- 試作の日本語のコメントを、本体のコメントと同じ規則 (なぜを書く) で書き直す

- [ ] **Step 5: Verus を流すスクリプトを書く**

`crates/eml_rt/verus/check.sh` (実行の権限を付ける):

```sh
#!/bin/sh
# eml_rt の Verus の写しを検証する (docs/implementation/testing.md の「Verus」)。Verus は rustup の toolchain を使うので、
# Nix の devShell の外の環境で流す。
set -eu
VERUS="${VERUS:-$HOME/.verus/verus-arm64-macos/verus}"
VERSION=0.2026.10.04.426d8b0
cd "$(dirname "$0")"
run() {
    env -i HOME="$HOME" PATH="$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin" "$VERUS" "$@"
}
run --version | grep -q "$VERSION" || {
    echo "check.sh: Verus $VERSION is required (docs/implementation/testing.md)" >&2
    exit 1
}
run --crate-type=lib lib.rs "$@"
```

Run: `crates/eml_rt/verus/check.sh`
Expected: `verification results:: N verified, 0 errors` (N は 18 前後)

- [ ] **Step 6: 照合と、証明が空でないことを確かめる**

Run: `cargo test -p eml_rt verus_copies`
Expected: 5つのテストが通る。`the_verus_copies_match_the_code` が落ちたら、写しを本体に合わせる (本体の動作は変えない)

次の2つを一時的に試し、それぞれ失敗することを確かめてから元に戻す。報告に結果を書く。

- 写しの `lin` の枝の `graph.push_children(node, stack);` を消す。`check.sh` が `invariant not satisfied` で落ち、`cargo test -p eml_rt verus_copies` も落ちる
- 写しの `well_formed` の、`lin` の物体の参照がただ1つである条件を消す。`check.sh` が落ちる

- [ ] **Step 7: 全体を確かめる**

Run: `cargo test -p eml_rt && cargo clippy -p eml_rt --all-targets -- -D warnings && cargo fmt --check && cargo test -p eml_cli --test integration citations`
Expected: 通る

- [ ] **Step 8: コミット**

```bash
git add -A crates/eml_rt
git commit -m "Prove share_walk in Verus against a checked copy

The walk is proved over the Graph trait's contract for graphs of any
size: it terminates, returns Ok, moves reached lin objects to the
receiver, marks reached local objects, leaves the rest alone, and keeps
shared objects from pointing at local non-lin ones. The proof lives in a
Verus copy outside the Cargo build; a unit test checks that the copy,
with Verus-only syntax removed, is the code. The loop is written with
loop/match so the copy can name the stack before each pop.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YLHZowtbHVXEbsTMFYwSMN"
```

---

### Task 9: Verus の証明 (ヘッダ、即値、RC の遷移、判定)

**Files:**
- Modify: `crates/eml_rt/src/word.rs` (ビット列の計算を整数の純粋な関数に分ける)
- Modify: `crates/eml_rt/src/meta.rs`、`crates/eml_rt/src/rc.rs`、`crates/eml_rt/src/heap/check.rs` (Verus が扱えない書き方を、意味を変えずに書き直す。Kani を指すコメント)
- Create: `crates/eml_rt/verus/meta.rs`、`crates/eml_rt/verus/word.rs`、`crates/eml_rt/verus/check.rs`
- Modify: `crates/eml_rt/verus/rc.rs`、`crates/eml_rt/verus/lib.rs`、`crates/eml_rt/src/verus_copies.rs` (`COPIES` の行)

**Interfaces:**
- Consumes: Task 8 の `crates/eml_rt/verus/`、`check.sh`、`COPIES`
- Produces: spec の「Verus」の証明の1、2、3、5

spec の「Verus」の証明の1、2、3、5を足す。証明の言い回しは spec のとおりにし、次に写しの形と、示すことを書く。写しの側に、本体にない `spec fn`、`proof fn`、証明のための exec の関数を置いてよい。表に載せない型 (`HeapError` など) は、写しの側で証明に要る部分だけを持てばよい。

Verus が扱えない書き方 (`Range::contains`、`bool::then`、書式つきの `unreachable!` など) は、本体を意味を変えずに Verus が扱える書き方にし、写しも同じにする。どう書いても Verus が扱えない関数は、その関数を表から外し、何を外したかと理由を報告する。`#[inline]` などの属性は照合で除かれるので、本体に残す。

- [ ] **Step 1: 即値を整数の関数に分ける**

`Word` はポインタ型なので Verus で扱えない。`word.rs` に、ビット列の計算を受け持つ整数の関数を置き、`Word` のメソッドはそれを呼ぶ形にする。名前と形は次のとおりにする。

```rust
/// 63 ビットに収まるか。
pub(crate) const fn fits_imm(n: i64) -> bool {
    Word::MIN_INT <= n && n <= Word::MAX_INT
}

/// 即値のビット列 `(n << 1) | 1`。
pub(crate) const fn imm_bits(n: i64) -> usize {
    ((n << 1) | 1) as usize
}

pub(crate) const fn is_imm_bits(bits: usize) -> bool {
    bits & 1 == 1
}

/// ポインタの語のビット列か。空の語 (0) はポインタでない。
pub(crate) const fn is_ptr_bits(bits: usize) -> bool {
    bits & 1 == 0 && bits != 0
}

/// 即値の値。算術右シフトで1ビットずらす。
pub(crate) const fn imm_value_bits(bits: usize) -> i64 {
    (bits as i64) >> 1
}
```

`Word::imm` は `assert!(fits_imm(n))` の後に `imm_bits(n)` を使い、`Word::int` は `fits_imm` を、`is_imm`、`is_ptr`、`imm_value` はそれぞれの関数を `self.0.addr()` に使う。`is_ptr` の `!self.0.is_null()` は `addr() != 0` と同じである。`MIN_INT` と `MAX_INT` は `impl Word` に残す。写しの側では、`impl Word` に2つの定数だけを持つ `Word` を置く。

Run: `cargo test -p eml_rt word`
Expected: 通る (動作は変わらない)

- [ ] **Step 2: 写しと証明を書く**

`crates/eml_rt/verus/lib.rs` に `mod check; mod meta; mod word;` を足し、`crates/eml_rt/src/verus_copies.rs` の `COPIES` に次の行を足す。名前は照合のテストの書き方 (`型::名前` は `impl 型` の中の項目) に従う。

| 本体 | 写し | 項目 |
|---|---|---|
| `meta.rs` | `../verus/meta.rs` | `Kind`、`Kind::from_bits`、`Meta`、`Meta::LIN`、`Meta::KIND_SHIFT`、`Meta::KIND_MASK`、`Meta::new`、`Meta::tag`、`Meta::scan`、`Meta::kind`、`Meta::lin`、`Meta::with_lin`、`Meta::same_shape` |
| `word.rs` | `../verus/word.rs` | `Word::MIN_INT`、`Word::MAX_INT`、`fits_imm`、`imm_bits`、`is_imm_bits`、`is_ptr_bits`、`imm_value_bits` |
| `rc.rs` | `../verus/rc.rs` | Task 8 の行に `LOCAL_MAX`、`DUP_LIMIT`、`DupStep`、`DecrefStep`、`dup_step`、`decref_step` を足す |
| `heap/check.rs` | `../verus/check.rs` | `layout_check`、`reuse_in_place_check` |

示すことは次のとおりである。

1. ヘッダの詰め方
   - 種類のビット (`(m >> KIND_SHIFT) & KIND_MASK`) が 0、1、2、15 のどれかであることを、`Meta` の正しさ (`spec fn`) にする。`new` と `with_lin` の結果は正しく、`kind` は正しい `Meta` を要る。`Kind::from_bits` の `unreachable!` の枝に届かないことは、この要件から出る
   - 任意の `tag`、`scan`、`kind`、`lin` で、`Meta::new(tag, scan, kind, lin)` の `tag()`、`scan()`、`kind()`、`lin()` が元の値に戻る
   - `m.with_lin(b)` は、`lin()` が `b` で、`tag()`、`scan()`、`kind()` が `m` と同じである
   - `a.same_shape(b)` は、`a` と `b` の `tag()`、`scan()`、`kind()` が同じときだけ真である
2. 即値
   - `fits_imm(n)` は、n が `MIN_INT` 以上 `MAX_INT` 以下のときだけ真である
   - `fits_imm(n)` なら、`imm_bits(n)` は `is_imm_bits` が真、`is_ptr_bits` が偽で、`imm_value_bits(imm_bits(n)) == n` である
   - `is_imm_bits` と `is_ptr_bits` が同時に真になるビット列はなく、0 はどちらでもない
3. RC の遷移。spec の「Verus」の3の対応を `spec fn` で書き、1回の遷移が対応を保つことを `proof fn` で示す。対応の状態は、rc、数 (`nat`)、張り付きの旗である。遷移は、Kani のハーネスと同じく次の3つである
   - `dup`: 数を1増やす。`dup_step` が `Store(n)` なら rc を n に、`Shared` なら rc を1減らし、`Skip` なら変えない
   - `decref` (数が正のときだけ): 数を1減らす。`Store(n)` なら rc を n に、`Free` なら解放、`Shared` なら rc を1増やして 0 になれば解放、`Skip` なら変えない
   - 印付け: `mark_step` が `Some(n)` なら rc を n にする
   - 遷移の後、rc が `LOCAL_MAX` か `DROP_LIMIT` より小さければ旗を立てる。示すことは、張り付いていなければ遷移の後も符号を除いた rc が数に一致して数が正であること、解放するのは張り付いていない物体の数が 0 になったときだけであること、rc が 0 なら3つの遷移のどれも rc を変えず解放もしないことである。始めの状態は、rc が 0 でなく、数が符号を除いた rc で、旗は rc が `LOCAL_MAX` か `DROP_LIMIT` より小さいときに立つ
   - `rc - 1` と `rc + 1` が桁あふれしないことも示す (`Shared` の枝は `DUP_LIMIT` 以上か `DROP_LIMIT` 以上で、負である)
4. 判定 (spec の5)
   - `layout_check(m, scanned, tag, keep_len)` が `Ok` になるのは、`m.kind()` が `Data`、`m.tag() == tag`、`scanned == keep_len` のときだけである
   - `reuse_in_place_check(boxed, new)` が `Ok` になるのは、`boxed.same_shape(new)` で、`boxed.lin() && !new.lin()` でないときだけである。`Err` の値は、形の違いなら `WrongMeta`、`lin` ビットを落とすなら `LinBitKept` である

Run: `crates/eml_rt/verus/check.sh`
Expected: `0 errors`

Run: `cargo test -p eml_rt verus_copies`
Expected: 通る

- [ ] **Step 3: 証明が空でないことを確かめる**

次の3つを一時的に試し、`check.sh` が落ちることを確かめてから元に戻す。報告に結果を書く。

- `Meta::new` の `KIND_SHIFT` を 23 にする
- `imm_value_bits` の算術右シフトを、`u64` の論理右シフトにする (`((bits as u64) >> 1) as i64`)
- `decref_step` の `rc == 1` の枝を `rc == 2` にする

- [ ] **Step 4: Kani を指すコメントを直す**

`rc.rs` の module のコメントの「ヒープ、単体テスト、Kani が同じ関数を使う」を「ヒープと単体テストが同じ関数を使い、Verus が写しで同じ関数を証明する」にする。`heap/check.rs` の「Kani が同じ関数を確かめる」の2か所を「Verus が写しで同じ関数を証明する」にする。

Run: `grep -rn 'Kani\|kani' crates/eml_rt`
Expected: 何も出ない

- [ ] **Step 5: 全体を確かめる**

Run: `cargo test -p eml_rt && cargo clippy -p eml_rt --all-targets -- -D warnings && cargo fmt --check && cargo test -p eml_cli --test integration citations && crates/eml_rt/verus/check.sh`
Expected: 通る

- [ ] **Step 6: コミット**

```bash
git add -A crates/eml_rt
git commit -m "Prove the header, immediates, RC transitions, and reuse checks in Verus

The proofs cover every value: Meta packing round-trips, immediates hold
exactly the 63-bit integers, each RC transition keeps the count model
(so any sequence does), and the release and reuse checks accept exactly
the named shapes. Word's bit arithmetic moves into integer functions so
the copies can state it without pointers.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YLHZowtbHVXEbsTMFYwSMN"
```

---

### Task 10: Miri と、差し替えの層を消す

**Files:**
- Delete: `crates/eml_rt/src/sync.rs`
- Modify: `crates/eml_rt/src/lib.rs` (`mod sync;` を消す)
- Modify: `crates/eml_rt/src/{alloc,meta,rc}.rs`、`crates/eml_rt/src/heap/{mod,check,reuse,share,walk}.rs`、`crates/eml_rt/src/heap/tests/share.rs` (`crate::sync` を `std` に)
- Modify: `crates/eml_rt/Cargo.toml` (`[lints.rust]` の `check-cfg` を消す)
- Modify: Miri が誤りを見つけたら、その原因のファイル

**Interfaces:**
- Consumes: Task 2 から Task 9 のすべて
- Produces: Miri の2つの設定で通る単体テスト。`sync` の module はなくなる

- [ ] **Step 1: Miri を strict provenance で流す**

Run: `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt`
Expected: すべてのテストが通る。Miri が UB を報告したら、報告の場所から原因を探して直し、その形を起こす最小の単体テストを、原因のファイルのテストに足す。テストを Miri から外して済ませない

- [ ] **Step 2: Miri を Tree Borrows で流す**

Run: `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_rt`
Expected: すべてのテストが通る。誤りの扱いは Step 1 と同じ

- [ ] **Step 3: 差し替えの層を消す**

spec の「`unsafe` の核」のとおり、atomic と確保の関数は std のものを直接使う。`crates/eml_rt/src/sync.rs` を消し、`lib.rs` の `mod sync;` を消す。`use crate::sync::…` を、同じ名前の `std::alloc::…` か `std::sync::atomic::…` に置き換える (`heap/tests/share.rs` の `Ordering as AtomicOrdering` の別名は残す)。`Cargo.toml` の `[lints.rust]` の節を消す (`cfg(loom)` も `cfg(kani)` も使わない)。

`meta.rs` の `HEADER_WORDS` のコメントを「ヘッダの語の数。8 と書かずに大きさから出し、ヘッダの形を変えたときに数え直す所を1か所にする。」にする。

Run: `grep -rn 'sync::\|loom\|同期の層' crates/eml_rt`
Expected: `std::sync::atomic` の行のほかは何も出ない

- [ ] **Step 4: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check && cargo test -p eml_cli --test integration citations && nix build && crates/eml_rt/verus/check.sh`
Expected: どれも通る。`verus_copies` のテストは、写しの関数の本体が `sync` を名指さないので変わらない

- [ ] **Step 5: コミット**

```bash
git add -A crates/eml_rt
git commit -m "Check eml_rt under Miri and drop the sync layer

The unit tests pass under Miri with strict provenance and with Tree
Borrows. The sync module existed to swap in loom's types; Verus checks
copies instead, so the core uses std's atomics and allocator directly.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YLHZowtbHVXEbsTMFYwSMN"
```

Miri の誤りを直したときは、このコミットとは別に、直したことを1行目に書いたコミットにする。

---

### Task 11: 計測

**Files:**
- Create: `crates/eml_rt/examples/rt_bench.rs`
- Create: `bench/rt.sh`

**Interfaces:**
- Consumes: `eml_rt` の公開の API (`Heap<Fast>`、`Meta`、`Kind`、`Word`、`Token`)
- Produces: `bench/rt.sh` の出力 (Task 12 で `benchmarks.md` に記録する)

- [ ] **Step 1: 計測用のプログラムを書く**

`crates/eml_rt/examples/rt_bench.rs`:

```rust
//! `eml_rt` の速いモードの計測 (docs/implementation/benchmarks.md)。`bench/rt.sh` が release ビルドで走らせ、命令の数を
//! 測る。reuse analysis はまだないので、reuse の列は手で組む。reuse の効果の見込みを、パスを作る前に数字で持つためである。

use std::hint::black_box;

use eml_rt::{Fast, Heap, Kind, Meta, Word};

const CONS: Meta = Meta::new(1, 2, Kind::Data, false);
const CELLS: usize = 1_000_000;
const ROUNDS: usize = 10;

fn main() {
    let case = std::env::args().nth(1).expect("a case: empty, list, box-list, dup, map, map-reuse");
    match case.as_str() {
        "empty" => {}
        "list" => list(),
        "box-list" => box_list(),
        "dup" => dup(),
        "map" => map(false),
        "map-reuse" => map(true),
        _ => panic!("an unknown case {case}"),
    }
}

fn build(heap: &mut Heap<Fast>, n: usize) -> Word {
    let mut l = Word::imm(0);
    for i in 0..n {
        let cell = heap.alloc(2, CONS);
        unsafe {
            heap.set_field(cell, 0, Word::imm(i as i64)).unwrap();
            heap.set_field(cell, 1, l).unwrap();
        }
        l = cell;
    }
    l
}

/// cons セルのリストを作って捨てる。
fn list() {
    let mut heap = Heap::<Fast>::new();
    for _ in 0..ROUNDS {
        let l = build(&mut heap, CELLS);
        unsafe { heap.decref(black_box(l)) }.unwrap();
    }
}

enum BoxList {
    Nil,
    Cons(i64, Box<BoxList>),
}

/// 同じことを `std::alloc` を直接使う `Box` のリストで行う。比べる基準である。
fn box_list() {
    for _ in 0..ROUNDS {
        let mut l = BoxList::Nil;
        for i in 0..CELLS as i64 {
            l = BoxList::Cons(i, Box::new(l));
        }
        // 再帰する Drop でスタックを溢れさせないように、先頭から1つずつ外す
        let mut cur = black_box(l);
        while let BoxList::Cons(_, next) = cur {
            cur = *next;
        }
    }
}

/// 局所の物体の `dup` と `decref` の繰り返し。
fn dup() {
    let mut heap = Heap::<Fast>::new();
    let x = heap.alloc(2, CONS);
    unsafe {
        heap.set_field(x, 0, Word::UNIT).unwrap();
        heap.set_field(x, 1, Word::UNIT).unwrap();
        for _ in 0..CELLS * ROUNDS {
            heap.dup(black_box(x)).unwrap();
            heap.decref(black_box(x)).unwrap();
        }
        heap.decref(x).unwrap();
    }
}

/// `map (+1)` に当たる作り直し (逆順のリストを作る)。`reuse` なら、空けた cons の箱を次の cons に使う。
fn map(reuse: bool) {
    let mut heap = Heap::<Fast>::new();
    let mut l = build(&mut heap, CELLS);
    for _ in 0..ROUNDS {
        let mut acc = Word::imm(0);
        let mut cur = l;
        while cur.is_ptr() {
            unsafe {
                let head = heap.field(cur, 0).unwrap();
                let tail = heap.field(cur, 1).unwrap();
                let cell = if reuse {
                    let token = heap.release_reuse(cur, 1, &[false, true]).unwrap();
                    heap.con_at(token, 2, CONS).unwrap().0
                } else {
                    heap.release(cur, 1, &[false, true]).unwrap();
                    heap.alloc(2, CONS)
                };
                heap.set_field(cell, 0, Word::imm(head.imm_value() + 1)).unwrap();
                heap.set_field(cell, 1, acc).unwrap();
                acc = cell;
                cur = tail;
            }
        }
        l = acc;
    }
    unsafe { heap.decref(black_box(l)) }.unwrap();
}
```

- [ ] **Step 2: 計測のスクリプトを書く**

`bench/rt.sh` (`chmod +x bench/rt.sh`):

```bash
#!/usr/bin/env bash
# eml_rt の速いモードの計測用プログラム (crates/eml_rt/examples/rt_bench.rs) を release ビルドで3回ずつ走らせ、実行した
# 命令の数と実時間の中央値を Markdown の表で出す (docs/implementation/benchmarks.md)。測り方は bench/run.sh と同じで、
# macOS の `/usr/bin/time -l` の `instructions retired` を使う。
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "bench/rt.sh: macOS の /usr/bin/time -l の instructions retired を使うので、macOS でだけ動く" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# 中央値を1つの値に決めるため、奇数にする
runs=3

cargo build --release -p eml_rt --example rt_bench --manifest-path "$root/Cargo.toml" >&2
bin="$root/target/release/examples/rt_bench"

# `runs` 個の値の中央値
median() {
  printf '%s\n' "$@" | sort -n | sed -n "$(( (runs + 1) / 2 ))p"
}

echo "- 機種: $(sysctl -n hw.model) ($(sysctl -n machdep.cpu.brand_string))"
echo "- OS: macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
echo "- rustc: $(rustc -V)"
echo "- コミット: $(git -C "$root" describe --always --dirty --abbrev=7)"
echo
echo "| 計測 | instructions retired | 実時間 (s) |"
echo "|---|---:|---:|"
for case in empty list box-list dup map map-reuse; do
  instructions=()
  seconds=()
  for _ in $(seq "$runs"); do
    if ! report="$( { /usr/bin/time -l "$bin" "$case" >/dev/null; } 2>&1 )"; then
      echo "bench/rt.sh: $case が失敗した" >&2
      echo "$report" >&2
      exit 1
    fi
    count="$(awk '/instructions retired/ { print $1 }' <<<"$report")"
    elapsed="$(awk '/ real / { print $1 }' <<<"$report")"
    # 数を読めないと表に空の欄が混ざるので止める
    if [[ ! "$count" =~ ^[0-9]+$ || ! "$elapsed" =~ ^[0-9]+\.[0-9]+$ ]]; then
      echo "bench/rt.sh: $case の /usr/bin/time -l の出力から命令の数か実時間を読めなかった" >&2
      echo "$report" >&2
      exit 1
    fi
    instructions+=("$count")
    seconds+=("$elapsed")
  done
  echo "| \`$case\` | $(median "${instructions[@]}") | $(median "${seconds[@]}") |"
done
```

- [ ] **Step 3: 流す**

Run: `bench/rt.sh`
Expected: 6行の表が出る。`list` が `box-list` より命令の数が少ない (自前の確保器が malloc より速い) こと、`map-reuse` が `map` より少ないことを見る。逆になったら、数を添えて報告する (計測の結果で設計を見直すかを相談する)。出力は Task 12 で使うので、そのまま書き留める

- [ ] **Step 4: clippy とコミット**

Run: `cargo clippy -p eml_rt --all-targets -- -D warnings && cargo fmt --check`

```bash
git add crates/eml_rt/examples bench/rt.sh
git commit -m "Add the eml_rt fast-mode measurement

bench/rt.sh measures instructions retired for building and freeing cons
lists against a Box list baseline, a dup/decref loop, and a map-like
rebuild with and without hand-built reuse.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YLHZowtbHVXEbsTMFYwSMN"
```

---

### Task 12: 文書

**Files:**
- Create: `docs/spec/object-model.md`
- Modify: `docs/README.md`
- Modify: `docs/future/roadmap.md`
- Modify: `docs/future/multicore.md`
- Modify: `docs/implementation/testing.md`
- Modify: `docs/implementation/benchmarks.md`
- Modify: `CLAUDE.md`
- Modify: `crates/eml_rt/**/*.rs` (引用のパス)
- Delete: `docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md`
- Delete: `docs/superpowers/plans/2026-10-13-s9a-eml-rt-core.md`

**Interfaces:**
- Consumes: Task 8 の Verus の導入と `check.sh`、Task 8 と Task 9 の報告 (写しにした関数、外した関数、spec からのずれ)、Task 11 の計測の表
- Produces: S9a の決めたことの規範の文書

文書を書く前に `yomiyasu:yomiyasu` のスキルを呼ぶ。

- [ ] **Step 1: `docs/spec/object-model.md` を書く**

位置づけは「規範」。冒頭で、`eml_rt` の核 (語、物体、確保器、RC、`share`、`lin` ビット、核の操作、reuse の部品、検査モード) を定めること、`eml_rt` はまだパイプラインから使わず、今のインタプリタのヒープは [ランタイム](runtime.md) が定めること、S10 で2つを合わせることを書く。見出しは次の並びにし、中身は spec の同じ見出しの節から、作業の設計の言い回し (「S9a では」「S9b で決める」の類) を規範の文に直して移す。S9b 以降に残すものは、各節の最後に「まだ決めていないこと」として [ロードマップ](../future/roadmap.md) の S9 の節を指す。

```
# オブジェクトモデル
## 語
## ダミーの値
## 物体
### ヘッダ
### ペイロード
### 種類
## 確保器
### ページ
### 確保と解放
### 生存の列挙
## 参照カウント
### 符号
### 局所の速い道
### 上限
### 共有の側
### 遷移の関数
## `share`
## `lin` ビット
## 核の操作
## reuse
## 検査モード
### モードの選び方
### 前置き
### 検査モードの token
### 不死の物体の数え方
### 報告する誤り
### 論理的な worker ID
## `unsafe` の核
## 受け入れる制約
```

spec からずれた所は、実装に合わせて書く。

- 「核の操作」の `alloc` と `con_at` はペイロードの語の数を受け取る (spec にすでにある)
- `Kind` の欄は型ごとの表でなく `match` で決める
- 検査モードの token の表は添字を使い回さない
- 検証の節 (単体テスト、Miri、Verus、計測) は移さない。`testing.md` と `benchmarks.md` に書く (Step 4)

- [ ] **Step 2: コメントの引用を移す**

Run: `grep -rl 'docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md' crates/eml_rt | xargs sed -i '' 's#docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md#docs/spec/object-model.md#g'`
Run: `grep -rl 'spec の「Verus」' crates/eml_rt | xargs sed -i '' 's#spec の「Verus」#docs/implementation/testing.md の「Verus」#g'`
Run: `grep -rl 'spec の「' crates/eml_rt | xargs sed -i '' 's#spec の「#docs/spec/object-model.md の「#g'`
Run: `grep -rn 'spec の「\|s9a-eml-rt-core' crates/eml_rt`
Expected: 何も出ない (`docs/spec/object-model.md の「` は `spec の「` を含まない)
Run: `cargo test -p eml_cli --test integration citations`
Expected: 通る。見出しのない引用と言われたら、Step 1 の見出しの綴り (バッククォートを含む) を spec と合わせる

- [ ] **Step 3: ロードマップと multicore.md を直す**

`docs/future/roadmap.md` の「S9 共通ランタイム `eml_rt`」の節:

- 「前提: S8。」の後に、S8 の節と同じ形の作業の表を足す。S9a の行は「語、ヘッダ、ページの確保器、局所と共有の RC、`share`、`lin` ビット、reuse の部品、検査モード、Miri と Verus」で、状態は「終えた。決めたことは [オブジェクトモデル](../spec/object-model.md) に移した。`eml_rt` はまだパイプラインから使わないので、UI テストの出力と `bench/` の仕事の回数は変わらない ([計測の基準](../implementation/benchmarks.md) の「S9a (`eml_rt` の核)」)」。S9b の行は「物体の種類ごとの中身 (データの物理的な形と、その規則を出す関数の置き場所、文字列、クロージャと `copy`、evidence の物体、`File`)、`Ctx`、`FnEntry`、`CodeBase`、語の ABI の extern と差分テスト」で、状態は「これから」
- 「決めたこと」の項目のうち S9a で移したもの (語、`tobj` の `Int`、ヘッダ、記述子、RC の符号、共有の経路、reuse しないこと、`lin` ビット、検査モード、reuse の備え、ダミー、`unsafe` の核) は、項目を1行にまとめて [オブジェクトモデル](../spec/object-model.md) の該当の節を指す
- 「共有の物体から届くのは共有か不死の物体だけ」を「共有の物体から届くのは、共有か不死の物体か、`lin` ビットの立った局所の物体だけ」に改め、理由 (`poly` の親の中の `lin` の子) を [オブジェクトモデル](../spec/object-model.md) の「`share`」で指す
- 「論点」の確保器の選定、`tobj` の中の `enum` のタグの表し方、検証の道具の採否を消し、作業の表の後に「S9a で閉じた論点」として、決めたこと (自前のページの確保器、即値、Miri の2つの設定と Verus) を書く
- S9 の節の Kani の項目と、`cfg(loom)` の差し替えの層の項目を、spec の「Verus」と「`unsafe` の核」に合わせて改める。Kani の代わりに Verus で、値の全域と任意のグラフについて証明する。差し替えの層は置かない

`docs/future/multicore.md` の「`par` の印付け」と「テスト」で、共有の物体から届く物体を述べる文があれば、同じ形に改める。検証の節の Kani と loom の項目は、Verus に改める。並行の手順は VM の並列化のときに Verus で証明し、vstd の atomic は SeqCst だけを扱うので、atomic の順序は文書の議論と Miri に任せる (spec の「Verus」)。「確定した設計判断」の 7 の「ヒープの実装方式」は、[オブジェクトモデル](../spec/object-model.md) の「確保器」を指す。

- [ ] **Step 4: testing.md、benchmarks.md、README、CLAUDE.md を直す**

`docs/implementation/testing.md`:

- 「層ごとの方法」か新しい節「`eml_rt` の検証」に、単体テストの形 (`both_modes!` でモードを型の引数に取る関数を2つのテストにする、検査モードだけのテストは誤りごとに最小の列を1つ置く、`cfg!(miri)` で大きさを小さくする)、Miri の2つの設定を書く。その下に見出し「Verus」の小節を置き、spec の「Verus」の中身 (写しの置き場所、5つの証明、単体テストに任せるもの、写しの照合のテストの規則、信頼する部分、Task 8 の導入の手順と `check.sh`、並行の手順と atomic の順序の扱い) と、Task 8 と Task 9 の報告の写しにした関数と外した関数を書く
- 「よく使うコマンド」に次を足す

  ```sh
  nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt   # Miri (Tree Borrows は -Zmiri-tree-borrows を足す)
  crates/eml_rt/verus/check.sh                                                                # Verus の証明 (Nix の外の toolchain を使う)
  bench/rt.sh                                                                                 # eml_rt の速いモードの命令の数 (macOS、release)
  ```

`docs/implementation/benchmarks.md`:

- 「命令の数の測り方」に、`bench/rt.sh` が `crates/eml_rt/examples/rt_bench.rs` の6つの計測 (`empty`、`list`、`box-list`、`dup`、`map`、`map-reuse`) を測ることを足す
- 「記録」に「S9a (`eml_rt` の核)」を足し、Task 11 の出力の環境と表を写す。`list` と `box-list`、`map` と `map-reuse` の差を数で書く。`bench/run.sh` の表は、`eml run` を変えていないので測らないと書く

`docs/README.md` の文書の地図の `spec/runtime.md` の行の後に足す:

```
| [spec/object-model.md](spec/object-model.md) | 規範 | `eml_rt` の核: 語、ヘッダと種類、ページの確保器と生存の列挙、局所と共有の RC、`share`、`lin` ビット、核の操作、reuse の部品、検査モード、受け入れる制約 |
```

`CLAUDE.md` の Commands のコードブロックに、英語のコメントで足す:

```sh
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt   # Miri (add -Zmiri-tree-borrows for Tree Borrows)
crates/eml_rt/verus/check.sh         # Verus proofs of eml_rt (runs outside Nix; see testing.md)
bench/rt.sh                          # eml_rt fast-mode instruction counts (macOS)
```

CLAUDE.md の Architecture と、`docs/implementation/status.md`、`docs/implementation/architecture.md` は、段の規則どおり S9 の終わりに直すので、ここでは変えない。

- [ ] **Step 5: spec と計画を消す**

```bash
git rm docs/superpowers/specs/2026-10-13-s9a-eml-rt-core-design.md docs/superpowers/plans/2026-10-13-s9a-eml-rt-core.md
```

`grep -rn 's9a-eml-rt-core' docs crates CLAUDE.md` が何も出さないことを確かめる。

- [ ] **Step 6: 全体を確かめる**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check && cargo test -p eml_cli --test integration citations`
Expected: 通る

- [ ] **Step 7: コミット**

```bash
git add -A docs crates/eml_rt CLAUDE.md
git commit -m "Document the eml_rt core and delete the S9a design and plan

The decisions of S9a move to docs/spec/object-model.md; the roadmap gets
the S9a/S9b table and the closed points, the shared reachability rule now
allows local lin-bit children, and testing.md and benchmarks.md describe
Miri, Verus, and bench/rt.sh.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01YLHZowtbHVXEbsTMFYwSMN"
```
