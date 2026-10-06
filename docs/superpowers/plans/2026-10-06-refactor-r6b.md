# リファクタリング R6b 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Core IR の組み立てと走査の道具を1つにし、IR をテキストで読み書きできるようにし、`eml_interp` を分け、末尾呼び出しを simplify の後で作る。最後に R6 の作業用の文書を `docs/` に移して消す。

**Architecture:** `CExpr` の子と値の走査 (visitor) と、関数を組み立てる `FnBuilder` を `eml_core_ir` に置き、translate、simplify、Perceus、包む関数がそれを使う。パスの間でアリーナを根からの前順に組み直し (`compact`)、verifier が木であることを確かめる。`pretty` の表示に boxed の印とエフェクトの表を足し、それを読む `parse` で、手組みのアリーナのテストを IR のテキストに書き直す。

**Tech Stack:** Rust (edition 2024)、`insta` (インラインのスナップショット)、`cargo test`。

**Spec:** `docs/superpowers/specs/2026-10-06-refactor-r6-design.md` の2章 (R6b) と、文書の扱い (冒頭の「位置づけ」)。

## Global Constraints

- 互換性は気にしない。後方互換のための分岐や別名は作らない (CLAUDE.md)
- コードのコメントと `docs/` は日本語で書く。コメントは「なぜ」を書き、`docs/` の規則を指すときはパスを書く。日本語を書くときは `yomiyasu:yomiyasu` スキルの規則に従う (CLAUDE.md)
- 設計をテストに合わせて曲げない。テストの変更は種類1 (振る舞い)、種類2 (内部表現のスナップショット)、種類3 (期待値が同じ機械的な追随) に分け、種類1と種類2は `docs/implementation/test-changes.md` の「リファクタリング R6」の節に記録する (docs/implementation/testing.md)
- 種類1と種類2の変更は、spec の表と、この計画で名前を挙げたものだけにする。それ以外の期待値が変わったら、受け入れる前に作業を止めて相談する
- UI テストの出力は変えない
- 各タスクの終わりに `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` を通す。触った crate ごとに `cargo test -p <crate> --no-run` も通す (`eml_test_support` の段階の feature で、各 crate のテストは自分の段階だけで組み立つ)
- コミットのメッセージは英語で書き、末尾に次の2行を付ける:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
  ```
- `git diff` は外部ツールを使う設定なので、スクリプトで差分を見るときは `git diff --no-ext-diff` を使う

## spec からの補い

- spec 2.1 の `settle` は、`pipeline.rs` に同じ名前の関数 (`captures` を埋めて検査する) がすでにあるので、`compact` と呼ぶ。spec は「simplify の最後」に置くとしたが、パイプラインが各パスの後に `compact` をかける。どのパスの後でも木であることを verifier が確かめられ、simplify だけに置くより強いためである
- spec 2.1 の `FnBuilder` は、新しく関数を組み立てる口 (`FnBuilder`) と、組み立て済みの関数をその場で書き換える口 (`CoreFn::push`、`CoreFn::fresh_like`) の2つにする。simplify はアリーナをその場で書き換えるので、`FnBuilder` に移し替えると往復の手間だけが増えるためである
- `pretty` は、操作を1つ以上持つエフェクトの表を先頭に表示する。`parse` がエフェクトの番号と、操作が再開するか (`never`) を復元するためである。操作のない `IO` は表示しない
- 変数の表示は `名前` と `番号` を続けて書く (`s1`)。名前が数字で終わると切れ目が決まらないので、`parse` は末尾の数字をすべて番号とみなす。表示が同じなら往復は成り立つ
- 入口の関数は、`entry$main` という名前の関数があればそれ、なければ最初の関数とする。表示に入口の行を足さないためである
- アリーナの形そのもの (共有された式、たどれない式、索引の誤り) を確かめる verifier のテストは、テキストでは書けないので、`verify.rs` の中の単体テスト (`#[cfg(test)]`) でアリーナを組んで書く。spec の完了の条件の「アリーナを手で組み立てるテストがない」は、`tests/` の結合テストについての条件とする

## Review Focus

- 名前が数字で終わる変数 (`x1` の番号 3 が `x13` と表示される) を含む IR の往復: 表示が変わらない (Task 5 にテストを置く)
- 文字列定数の中の `"`、`\`、改行、Unicode を含む `const` の往復: 同じ文字列に戻る (Task 5 にテストを置く)
- 負の整数の値 (`-3`) を含む IR の往復: 同じ値に戻る (Task 5 にテストを置く)
- join point を B4 で消した関数の `compact`: 残った join point の番号は元の順のまま詰まる (Task 3 で既存の `simplify` のスナップショットが変わらないことで確かめる。加えて単体テストを置く)
- 非末尾の `if` の枝に動いた呼び出しが、末尾呼び出しになったうえで、深い再帰でもフレームを積まない: Core IR が `tailcall` になる (Task 4 にスナップショットを置く)

---

## ファイルの構成

| ファイル | 変更 | 担当するタスク |
|---|---|---|
| `crates/eml_core_ir/src/lib.rs` | `CExpr`、`Rhs`、`Call` の visitor。`VarInfo::linearity` を消す。`CoreFn::push`、`fresh_like` | 1、2、5 |
| `crates/eml_core_ir/src/builder.rs` (新規) | `FnBuilder` | 2 |
| `crates/eml_core_ir/src/compact.rs` (新規) | `compact` (前順の組み直しと join point の番号の振り直し) | 3 |
| `crates/eml_core_ir/src/text.rs` (新規) | `parse` と `ParseError` | 5 |
| `crates/eml_core_ir/src/simplify.rs` | visitor と `CoreFn` の口を使う。`renumber` を `compact` に移す。規則 T を足す | 1、2、3、4 |
| `crates/eml_core_ir/src/perceus.rs`、`liveness.rs`、`verify.rs`、`pretty.rs` | visitor を使う。verifier の木の検査。boxed の印とエフェクトの表 | 1、2、3、5 |
| `crates/eml_core_ir/src/pipeline.rs` | 各パスの後に `compact` | 3 |
| `crates/eml_core_ir/src/translate/mod.rs`、`program.rs`、`types.rs` | `FnBuilder` を使う。`tail_after` の `vars.pop` を消す | 2、4、5 |
| `crates/eml_core_ir/tests/*.rs` | スナップショットの追随、`verify.rs` のテキスト化、往復の確認 | 4、5、6 |
| `crates/eml_interp/src/` | `machine.rs`、`effects.rs`、`prim.rs`、`io.rs`、`error.rs` に分ける。`Prepared` を消す | 8 |
| `crates/eml_interp/tests/*.rs` | テキストの IR に書き直す | 7 |
| `crates/eml_test_support/src/lib.rs`、`tests/support.rs` | `ir` の組み立ての整理 | 5、7 |
| `crates/eml_types`、`crates/eml_hir` | R6a の残りの小さな片付け | 9 |
| `docs/` | 各タスクの文書。R6 の作業用の文書の削除 | 3〜10 |

---

### Task 1: `CExpr` の子と値の visitor (A5)

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs` (`CExpr::atoms_mut`、`Rhs::atoms`、`Rhs::atoms_mut`、`Call::atoms`、`Call::atoms_mut` のあたり)
- Modify: `crates/eml_core_ir/src/simplify.rs` (`children`、`replace_child`、`used_atoms` と、それらの呼び出し)
- Modify: `crates/eml_core_ir/src/liveness.rs`、`verify.rs`、`perceus.rs` (子をたどるだけの `match` があれば)

**Interfaces:**
- Produces (すべて `eml_core_ir` の中で使う。`pub(crate)` でよい。`Rhs::atoms` と `Call::atoms` は今の `pub` のまま):

```rust
impl CExpr {
    pub(crate) fn for_each_child(&self, f: impl FnMut(CExprId));
    pub(crate) fn for_each_child_mut(&mut self, f: impl FnMut(&mut CExprId));
    pub(crate) fn for_each_atom(&self, f: impl FnMut(Atom));
    pub(crate) fn for_each_atom_mut(&mut self, f: impl FnMut(&mut Atom));
}
impl Rhs {
    pub(crate) fn for_each_atom(&self, f: impl FnMut(Atom));
    pub(crate) fn for_each_atom_mut(&mut self, f: impl FnMut(&mut Atom));
}
impl Call {
    pub(crate) fn for_each_atom(&self, f: impl FnMut(Atom));
    pub(crate) fn for_each_atom_mut(&mut self, f: impl FnMut(&mut Atom));
}
```

- [ ] **Step 1: 順を固定する単体テストを書く**

`crates/eml_core_ir/src/lib.rs` の末尾に足す。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_and_atoms_come_in_a_fixed_order() {
        let join = CExpr::Join {
            join: JoinId(0),
            params: vec![VarId(0)],
            captures: vec![VarId(1)],
            body: CExprId(3),
            scope: CExprId(4),
        };
        let mut children = Vec::new();
        join.for_each_child(|child| children.push(child));
        assert_eq!(children, [CExprId(3), CExprId(4)]);

        let handle = CExpr::TailCall(Call::Handle {
            effect: 0,
            body: Atom::Var(VarId(1)),
            clauses: vec![Atom::Var(VarId(2)), Atom::Var(VarId(3))],
            ret: Some(Atom::Var(VarId(4))),
        });
        let mut atoms = Vec::new();
        handle.for_each_atom(|atom| atoms.push(atom));
        assert_eq!(atoms, [1, 2, 3, 4].map(|n| Atom::Var(VarId(n))));
        assert_eq!(atoms, Call::atoms(match &handle {
            CExpr::TailCall(call) => call,
            _ => unreachable!(),
        }));
    }
}
```

Run: `cargo test -p eml_core_ir --lib children_and_atoms`
Expected: FAIL (`for_each_child` がない)

- [ ] **Step 2: visitor を書く**

`lib.rs` の `impl CExpr` の `atoms_mut` を消し、次に置き換える。`..` を使わずにすべての欄を名前で受ける。欄を足したときに直し忘れないようにするためである (6b で `Handle` と `Resume` の形が変わる)。

```rust
impl CExpr {
    /// 子の式を、`Join` は本体、範囲の順に、`Switch` は枝の順に渡す。子をたどる処理はすべてここを通す。
    pub(crate) fn for_each_child(&self, mut f: impl FnMut(CExprId)) {
        match self {
            CExpr::Let { var: _, rhs: _, body }
            | CExpr::Dup { var: _, body }
            | CExpr::Decref { var: _, body } => f(*body),
            CExpr::Join {
                join: _,
                params: _,
                captures: _,
                body,
                scope,
            } => {
                f(*body);
                f(*scope);
            }
            CExpr::Switch { scrutinee: _, arms } => arms.iter().for_each(|arm| f(arm.body)),
            CExpr::Jump { join: _, args: _ } | CExpr::Return(_) | CExpr::TailCall(_) => {}
        }
    }

    pub(crate) fn for_each_child_mut(&mut self, mut f: impl FnMut(&mut CExprId)) {
        match self {
            CExpr::Let { var: _, rhs: _, body }
            | CExpr::Dup { var: _, body }
            | CExpr::Decref { var: _, body } => f(body),
            CExpr::Join {
                join: _,
                params: _,
                captures: _,
                body,
                scope,
            } => {
                f(body);
                f(scope);
            }
            CExpr::Switch { scrutinee: _, arms } => {
                arms.iter_mut().for_each(|arm| f(&mut arm.body))
            }
            CExpr::Jump { join: _, args: _ } | CExpr::Return(_) | CExpr::TailCall(_) => {}
        }
    }

    /// 式が直接使う値。子の式の値は含まない。
    pub(crate) fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            CExpr::Let { var: _, rhs, body: _ } => rhs.for_each_atom(f),
            CExpr::Switch { scrutinee, arms: _ } => f(*scrutinee),
            CExpr::Jump { join: _, args } => args.iter().for_each(|&atom| f(atom)),
            CExpr::Return(atom) => f(*atom),
            CExpr::TailCall(call) => call.for_each_atom(f),
            CExpr::Join { .. } | CExpr::Dup { .. } | CExpr::Decref { .. } => {}
        }
    }

    pub(crate) fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            CExpr::Let { var: _, rhs, body: _ } => rhs.for_each_atom_mut(f),
            CExpr::Switch { scrutinee, arms: _ } => f(scrutinee),
            CExpr::Jump { join: _, args } => args.iter_mut().for_each(f),
            CExpr::Return(atom) => f(atom),
            CExpr::TailCall(call) => call.for_each_atom_mut(f),
            CExpr::Join { .. } | CExpr::Dup { .. } | CExpr::Decref { .. } => {}
        }
    }
}
```

`for_each_atom` の最後の腕の `..` は、値を持たない種類をまとめているだけで、子の走査ではない。ただし `Dup` と `Decref` の `var` は「使用」なので、今の `atoms_mut` と同じく含めない (Perceus の命令の変数は値の使用として数えない)。

`Rhs` と `Call` にも同じ形の `for_each_atom` と `for_each_atom_mut` を書き、今の `atoms` (`Vec` を返す、`pub`) は `for_each_atom` で集める実装にする。`atoms_mut` は消す。順は今の `atoms` と同じ (関数値の呼び出しは呼ばれる値が先、handle は本体、節、`return` の節) にする。

- [ ] **Step 3: 呼び出し側を置き換える**

- `simplify.rs` の `children(expr)` は `expr.for_each_child(|c| …)` に、`replace_child(expr, old, new)` は `expr.for_each_child_mut(|slot| if *slot == old { *slot = new })` に、`used_atoms(expr)` は `expr.for_each_atom` に置き換え、3つの関数を消す。`replace_child` は親が子を指していることを `expect` していたので、置き換えた後に置き換えたかを数えて `debug_assert!` で確かめる
- `atoms_mut()` を使っていた箇所 (`simplify.rs` の `substitute` など) は `for_each_atom_mut` にする
- `liveness.rs`、`verify.rs`、`perceus.rs` で、子をたどるだけ・値を集めるだけの `match` があれば同じ visitor にする。種類ごとに意味の違う処理 (所有権の規則など) の `match` はそのまま残す

- [ ] **Step 4: テストを流してコミットする**

Run: `cargo test -p eml_core_ir && cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS。スナップショットは1つも変わらない (振る舞いを変えない作り替え)

```bash
git add -A crates
git commit -F - <<'EOF'
Walk Core IR children and atoms through one visitor

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 2: 関数の組み立ての口を1つにする (A5)

**Files:**
- Create: `crates/eml_core_ir/src/builder.rs`
- Modify: `crates/eml_core_ir/src/lib.rs` (`mod builder;`、`CoreFn::push`、`CoreFn::fresh_like`)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`FnLowering` の `exprs`、`vars`、`joins` と `push`、`new_var`、`new_join`、`seq`)
- Modify: `crates/eml_core_ir/src/translate/program.rs` (`CExprId(0)` などを手で書いて関数を組む箇所すべて)
- Modify: `crates/eml_core_ir/src/simplify.rs` (`push`、`fresh_like`)、`perceus.rs` (`Rebuild` の `new`、`joins`、`push`)

**Interfaces:**
- Produces:

```rust
/// 新しく関数を組み立てる口。変数、式、join point を足し、最後に `CoreFn` にする。
pub(crate) struct FnBuilder { /* vars, exprs, joins: Vec<Option<CExprId>> */ }
impl FnBuilder {
    pub(crate) fn new() -> FnBuilder;
    pub(crate) fn var(&mut self, info: VarInfo) -> VarId;
    pub(crate) fn push(&mut self, expr: CExpr) -> CExprId;
    pub(crate) fn new_join(&mut self) -> JoinId;
    /// `join` の定義の位置を索引に入れる。`push` した `Join` の式を指す。
    pub(crate) fn define_join(&mut self, join: JoinId, at: CExprId);
    /// 索引のすべての join point が定義済みであることを `expect` で確かめる。
    pub(crate) fn finish(self, name: String, params: Vec<VarId>, body: CExprId) -> CoreFn;
}
impl CoreFn {
    /// 組み立て済みの関数をその場で書き換えるパスのための口。
    pub(crate) fn push(&mut self, expr: CExpr) -> CExprId;
    pub(crate) fn fresh_like(&mut self, var: VarId) -> VarId;
}
```

- [ ] **Step 1: 単体テストを書く**

`builder.rs` の末尾に置く。

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Atom, CExpr};

    #[test]
    fn a_builder_makes_a_function_with_its_join_index() {
        let mut builder = FnBuilder::new();
        let x = builder.var(VarInfo {
            name: "x".to_string(),
            linearity: crate::Linearity::Unr,
            boxed: false,
        });
        let join = builder.new_join();
        let body = builder.push(CExpr::Return(Atom::Var(x)));
        let scope = builder.push(CExpr::Jump { join, args: vec![Atom::Int(1)] });
        let at = builder.push(CExpr::Join {
            join,
            params: vec![x],
            captures: Vec::new(),
            body,
            scope,
        });
        builder.define_join(join, at);
        let function = builder.finish("f".to_string(), Vec::new(), at);
        assert_eq!(function.join(join), (&[x][..], body));
    }
}
```

`VarInfo` は Task 5 で `linearity` を消すまで `linearity` を持つので、この Task のテストは `linearity` を書いて組む (Task 5 でその行を消す)。

Run: `cargo test -p eml_core_ir --lib a_builder_makes`
Expected: FAIL (`FnBuilder` がない)

- [ ] **Step 2: `FnBuilder` と `CoreFn` の口を書き、使う側を置き換える**

- `FnLowering` は `exprs`、`vars`、`joins` の代わりに `builder: FnBuilder` を持つ。`push`、`new_var` (`var_info` を作って `builder.var`)、`new_join` はそれに委ね、`seq` の `self.joins[...] = Some(expr)` は `define_join` にする。関数を `CoreFn` にする箇所は `finish` を使う
- `translate/program.rs` で、包む関数 (`op$…`、`con$…`、`builtin$…`) と入口の関数 (`entry$main`) を組む箇所は、すべて `FnBuilder` で組む。`CExprId(0)` や `CExprId(1)` のような番号を手で書かない
- `simplify.rs` の `push` と `fresh_like` は `CoreFn::push` と `CoreFn::fresh_like` を呼ぶ
- `perceus.rs` の `Rebuild` は、新しいアリーナと join point の索引を `FnBuilder` で持つ。最後に今と同じく `function.exprs` と `function.joins` を差し替える (変数の表はそのまま使う)

- [ ] **Step 3: テストを流してコミットする**

Run: `cargo test -p eml_core_ir && cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS。スナップショットは変わらない

```bash
git add -A crates
git commit -F - <<'EOF'
Build Core IR functions through one builder

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 3: パスの間でアリーナを組み直し、木であることを確かめる (A5)

**Files:**
- Create: `crates/eml_core_ir/src/compact.rs`
- Modify: `crates/eml_core_ir/src/pipeline.rs` (`settle` と Perceus の後)
- Modify: `crates/eml_core_ir/src/simplify.rs` (`renumber` を消し、doc コメントの「Perceus がアリーナを作り直すときに捨てる」を直す)
- Modify: `crates/eml_core_ir/src/verify.rs` (木の検査と、その単体テスト)
- Modify: `docs/spec/core-ir.md`、`docs/implementation/architecture.md`

**Interfaces:**
- Produces: `pub(crate) fn compact(function: &mut CoreFn)`。根から前順 (式、子の順) で式を並べ直し、たどれない式を捨て、残った join point に元の番号の順で 0 から番号を振り直して索引を作り直す

- [ ] **Step 1: 失敗するテストを書く**

`compact.rs` の単体テストに、たどれない式と消えた join point を持つ関数を `FnBuilder` で組み、`compact` の後に (a) `exprs.len()` が根からたどれる式の数と同じ、(b) 根が `CExprId(0)`、(c) 残った join point の番号が 0 から詰まり、`join(JoinId(0))` が残ったほうの本体を指す、を確かめるテストを書く。

`verify.rs` の単体テスト (`#[cfg(test)]`) に、2つの親から指される式を持つ関数と、たどれない式を持つ関数を組み、`verify_scopes` がそれぞれ `"expression e… is reachable twice"`、`"expression e… is not reachable from the body"` を含む誤りを返すテストを書く。

Run: `cargo test -p eml_core_ir --lib`
Expected: FAIL (`compact` がなく、verifier が木を確かめない)

- [ ] **Step 2: `compact` と木の検査を書く**

- `compact` は作業の列で前順にたどる (長い連鎖で再帰しない)。古い ID から新しい ID への表を作り、子の ID を `for_each_child_mut` で書き換える。join point の番号は、残ったものに元の番号の小さい順で振り直し、`Join` と `Jump` の `join` を書き換える。今の `simplify::renumber` の規則と同じなので、その関数は消す
- verifier の2つの入口 (`verify`、`verify_scopes`) は、最初に木の検査をする。根から作業の列でたどり、2回目に来た式があれば `expression e{N} is reachable twice`、たどった数が `exprs.len()` より少なければ、最初のたどれない式について `expression e{N} is not reachable from the body` の誤りにする
- `pipeline.rs` は、translate、simplify、Perceus のそれぞれの後に、関数ごとに `compact` をかけてから今の `liveness::analyze` と検査を行う

- [ ] **Step 3: テストを流す**

Run: `cargo test -p eml_core_ir && cargo test`
Expected: PASS。`pretty` は根からたどり、join point の番号の振り方も今と同じなので、スナップショットは変わらない。変わったら止めて相談する

- [ ] **Step 4: 文書を直してコミットする**

- `docs/spec/core-ir.md` のパスの節に、各パスの後にアリーナを根からの前順に組み直し、たどれない式を捨て、join point の番号を元の順で詰めることと、verifier がどの式も根からちょうど1回たどれることを確かめることを書く
- `docs/implementation/architecture.md` の「木から外れた式はアリーナに残るが、Perceus が捨てる」と「最後に、残った join point に元の順で番号を振り直す」を、`compact` の説明に直す。`simplify.rs` の先頭の doc コメントも同じく直す

Run: `cargo clippy --all-targets && cargo fmt --check`

```bash
git add -A crates docs
git commit -F - <<'EOF'
Compact Core IR arenas between passes and verify they are trees

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 4: 末尾呼び出しを simplify の後で作る (A7)

**Files:**
- Modify: `crates/eml_core_ir/src/simplify.rs` (規則 T と、先頭の doc コメントのパスの順)
- Modify: `crates/eml_core_ir/src/translate/mod.rs:308-323` (`tail_after`)
- Test: `crates/eml_core_ir/tests/simplify.rs`、既存のスナップショット
- Modify: `docs/spec/core-ir.md`、`docs/implementation/test-changes.md`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/simplify.rs` に足す。

```rust
#[test]
fn a_call_moved_into_a_branch_becomes_a_tail_call() {
    let text = "g : Int -> Int\ng x = x\n\nh : Int -> Int\nh x = x\n\nf : Bool -> Int -> Int\nf c x =\n  let y = if c then g x else h x\n  y\n\nmain : Unit -> <IO> Unit\nmain () = println (show_int (f True 1))";
    insta::assert_snapshot!(core_text(text, Pass::Simplify), @"");
}
```

Run: `cargo test -p eml_core_ir --test simplify a_call_moved`
Expected: FAIL。`cargo insta review` で、`f` の両方の枝が `let t… = call g(x1)` と `return t…` になっていることを見る (受け入れない)

- [ ] **Step 2: 規則 T を足し、translate の詰め処理を消す**

`simplify.rs` に足し、`simplify` の最後 (`remove_dead_bindings` の後) で呼ぶ。

```rust
    /// T: 呼び出しの結果をそのまま返す `let x = call …` と `return x` を末尾呼び出しにする。末尾呼び出しを作る場所を
    /// ここ1か所にする。B3 や B5 が枝へ動かした呼び出しも、ここで末尾呼び出しになる (docs/spec/core-ir.md)。
    fn tail_calls(&mut self) {
        for id in self.reachable() {
            let CExpr::Let { var, rhs: Rhs::Call { call, .. }, body } = self.expr(id) else {
                continue;
            };
            if self.expr(*body) == &CExpr::Return(Atom::Var(*var)) {
                let call = call.clone();
                self.set(id, CExpr::TailCall(call));
            }
        }
    }
```

`translate/mod.rs` の `tail_after` は、`self.tail_expr` で作った最後の命令を `self.seq(bindings, last)` に渡すだけにする (`vars.pop` と `TailCall` への書き換えを消す)。doc コメントの「値を返すだけの呼び出しは、呼び出し元のフレームを積まない末尾呼び出しにする」は「末尾呼び出しは simplify の T が作る」に直す。`simplify.rs` の先頭の doc コメントのパスの順に T を足す。

- [ ] **Step 3: テストを流し、変わる期待値を確かめる**

Run: `cargo test -p eml_core_ir`
Expected: 次の種類2の変化だけが出る。中身を見てから受け入れる (`cargo insta review`、インラインのスナップショットは期待値を直す)。

- 新しいテスト: `f` の2つの枝が `tailcall g(x1)` と `tailcall h(x1)` になる
- `Pass::Translate` で止めるスナップショット (`tests/translate.rs`) のうち `tailcall` を含むもの: `let t… = call …` と `return t…` の2行になる
- `Pass::Simplify` と `Pass::Perceus` のスナップショットのうち、translate が詰めていた変数の番号の後ろで新しい変数を作るもの: 変数の番号が1つずれる
- 今まで `call` と `return` が並んでいた枝 (不具合4の形) が `tailcall` になるもの

これ以外の変化 (UI テストを含む) があれば止めて相談する。

Run: `cargo test`
Expected: PASS

- [ ] **Step 4: 文書を直してコミットする**

- `docs/spec/core-ir.md` のパスの順に T を足し、末尾呼び出しを T が作ることを書く
- `docs/implementation/test-changes.md` の「リファクタリング R6」に、変わったスナップショットの名前を種類ごとに並べる (種類2)。新しいテストも書く

Run: `cargo clippy --all-targets && cargo fmt --check`

```bash
git add -A crates docs
git commit -F - <<'EOF'
Form tail calls after simplify so moved calls become tail calls

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 5: Core IR のテキストを読めるようにする (A5)

**Files:**
- Create: `crates/eml_core_ir/src/text.rs`
- Modify: `crates/eml_core_ir/src/lib.rs` (`pub use text::{ParseError, parse};`、`VarInfo::linearity` と `pub use eml_types::Linearity` を消す)
- Modify: `crates/eml_core_ir/src/pretty.rs` (boxed の印、エフェクトの表)
- Modify: `crates/eml_core_ir/src/liveness.rs:17`、`translate/types.rs:38` (`linearity` の削除)
- Modify: `crates/eml_core_ir/tests/common/mod.rs` (往復の確認)
- Modify: `crates/eml_core_ir/tests/{translate,simplify,perceus}.rs` (スナップショットの追随)
- Modify: `crates/eml_test_support/src/lib.rs` (`ir::var_info`)、`crates/eml_test_support/tests/support.rs`
- Modify: `docs/spec/core-ir.md`、`docs/implementation/test-changes.md`

**Interfaces:**
- Produces:

```rust
pub fn parse(text: &str) -> Result<Program, ParseError>;
pub struct ParseError { pub line: usize, pub message: String }   // Display: "line {line}: {message}"
```

#### 表示の形 (`pretty`) の変更

- boxed の変数には、束縛の位置 (関数の引数、`let`、join point の引数、`switch` の枝のフィールド) で名前の後に `^` を付ける。例: `fn twice(s0^) {`、`let s1^ = const "a"`、`join j0(t3^) [s1] {`、`#1(x2^, x3) ->`。使用の位置 (値、`captures`、`saved`) には付けない
- 先頭に、操作を1つ以上持つエフェクトを表の順に1行ずつ表示する。`effect Ask { ask }`、`effect Fail { never fail }`、`effect Mixed { single, multi_op }` の形で、再開しない操作には `never` を付ける。操作のないエフェクト (`IO`) は表示しない。エフェクトの行と最初の関数の間に空行は入れない

#### 読む形 (`parse`)

`pretty` の出力をそのまま読める文法にする。行の字下げは見ない (`}` と終わりの命令で区切りが決まる)。

```
program  := effect* function*
effect   := 'effect' NAME '{' (['never'] NAME) (',' ['never'] NAME)* '}'
function := 'fn' FNAME '(' [binder (',' binder)*] ')' '{' chain '}'
binder   := VAR ['^']
chain    := stmt* last
stmt     := 'let' binder '=' rhs
          | 'dup' VAR | 'decref' VAR
          | 'join' 'j'N '(' [binder,*] ')' '[' [VAR,*] ']' '{' chain '}'     -- 続く stmt と last が範囲
last     := 'return' atom | 'jump' 'j'N '(' [atom,*] ')' | 'tailcall' call
          | 'switch' atom '{' arm* '}'
arm      := '#'N ['(' binder,* ')'] '->' chain
rhs      := atom | 'call' FNAME '(' atoms ')' [saved] | call [saved]
          | 'closure' FNAME '(' atoms ')' | 'prim' PRIM '(' atoms ')' | 'const' STRING
          | 'con' '#'N '(' atoms ')' | 'perform' IOOP '(' atoms ')' | 'drop' atom
call     := FNAME '(' atoms ')'                         -- tailcall の Direct
          | 'apply' atom '(' atoms ')'
          | 'handle' EFF '(' atom ')' '{' [OP ':' atom (',' OP ':' atom)*] '}' ['return' atom]
          | 'perform' EFF '.' OP '(' atoms ')'
          | 'resume' atom '(' atom ')'
saved    := '[' [VAR,*] ']'
atom     := VAR | INT | '()' | '#'N
```

- 名前の字は、英数字と `_`、`$`、`'` である。`FNAME` (関数の名前) は `(` の直前までの空白を含まない字の並びで、`builtin$++` や `<+>` も読む
- キーワード (`apply`、`handle`、`perform`、`resume`) の直後が `(` なら、それは同じ名前の関数の `Direct` の呼び出しとして読む
- `perform X.y(…)` はユーザーの操作、`perform name(…)` は IO の操作 (`IoOp` の名前) である。`OP` は名前のほか、壊れた IR のテストのために `#N` (操作の番号) でも書ける
- `VAR` は、末尾の数字の並びを番号 (`VarId`)、その前を名前とする。番号が飛んでいれば、その間は名前のない `boxed: false` の変数で埋める。同じ番号に違う名前か違う `^` が付いたら誤りにする
- `PRIM` は `PrimOp::name` の逆、`IOOP` は `IoOp::name` の逆で引く (`PrimOp::from_name`、`IoOp::from_name` を足す)
- `STRING` は Rust の `{:?}` の形 (`\"`、`\\`、`\n`、`\r`、`\t`、`\0`、`\u{…}`) を読む。文字列定数の表は現れた順に作り、同じ文字列は同じ番号にする
- 関数の番号は `fn` の現れた順で、呼び出しは名前で引く (2回読む)。入口は `entry$main` という名前の関数、なければ最初の関数
- 誤りは、行の番号と理由を持つ `ParseError` にする (知らない名前、閉じない括弧、終わりの命令がない連なり、など)

- [ ] **Step 1: 失敗するテストを書く**

`text.rs` の単体テストに足す (往復は `pretty(&parse(t)?) == t` で確かめる)。

```rust
    fn round_trip(text: &str) {
        let program = parse(text).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(crate::pretty(&program), text);
    }

    #[test]
    fn a_program_with_joins_switches_and_effects_round_trips() {
        round_trip(
            "effect Ask { ask, never stop }\n\
             fn pick(b0, s1^) {\n  join j0(t3^) [s1] {\n    let t4^ = prim ++(t3, s1)\n    return t4\n  }\n  switch b0 {\n    #0 ->\n      let s2^ = const \"none\"\n      jump j0(s2)\n    #1 ->\n      dup s1\n      jump j0(s1)\n  }\n}\n\
             fn entry$main() {\n  let c0^ = closure pick(#1)\n  let t1 = perform Ask.ask(()) [c0]\n  tailcall apply c0(-3)\n}\n",
        );
    }

    #[test]
    fn a_name_that_ends_with_a_digit_round_trips() {
        round_trip("fn f(x10^) {\n  return x10\n}\n");
    }

    #[test]
    fn string_escapes_round_trip() {
        round_trip("fn f() {\n  let s0^ = const \"a\\\"b\\\\c\\nd\\u{1f600}\"\n  return s0\n}\n");
    }

    #[test]
    fn an_unknown_function_is_an_error_with_its_line() {
        let error = parse("fn f() {\n  tailcall g(1)\n}\n").unwrap_err();
        assert_eq!(error.line, 2);
    }
```

`a_program_with_joins_switches_and_effects_round_trips` の文字列は、`pretty` の行の形 (字下げ2つずつ、`handle` と `perform` の書き方) に合わせる。最初に `pretty` を直したあと、表示の細部 (字下げの深さなど) が上と違えば、上の文字列を `pretty` の形に合わせる。構文の要素 (エフェクトの行、`^`、join point、`switch`、`perform`、`saved`、負の整数) は減らさない。

`crates/eml_core_ir/tests/common/mod.rs` の `core_text` を、表示を返す前に往復を確かめる形にする。Core IR のすべてのスナップショットのテストが往復のテストを兼ねる。

```rust
pub fn core_text(text: &str, last: Pass) -> String {
    let shown = eml_core_ir::pretty(&eml_test_support::core_until(text, last));
    let parsed = eml_core_ir::parse(&shown).unwrap_or_else(|error| panic!("{error}\n{shown}"));
    assert_eq!(eml_core_ir::pretty(&parsed), shown, "the printed Core IR must read back");
    shown
}
```

Run: `cargo test -p eml_core_ir --lib text`
Expected: FAIL (`parse` がない)

- [ ] **Step 2: `VarInfo::linearity` を消す**

`lib.rs` の `VarInfo::linearity` と `pub use eml_types::Linearity;` を消す。`liveness.rs` の `tracked` は `var.boxed` だけを見る。`translate/types.rs`、`eml_test_support::ir::var_info`、`eml_test_support/tests/support.rs`、Task 2 の `builder.rs` のテストの `linearity` を消す。doc コメントの「Kind とボックス化の有無だけを残す」は「ボックス化の有無だけを残す」に直す。

- [ ] **Step 3: `pretty` を直し、`parse` を書く**

上の「表示の形」と「読む形」のとおりにする。`parse` は、字句 (名前、数、文字列、記号) に分ける段と、行をまたいで再帰下降で読む段に分ける。式は子から先にアリーナに入れてよい (前順にするのは `compact` の役目で、`parse` の結果は検査やインタプリタにそのまま渡す)。

- [ ] **Step 4: テストを流し、スナップショットを追随させる**

Run: `cargo test -p eml_core_ir`
Expected: `text.rs` の単体テストが通る。`tests/{translate,simplify,perceus}.rs` のスナップショットは、boxed の印 `^` と、エフェクトを持つプログラムの先頭のエフェクトの行だけが変わる (種類2)。それ以外の差がないことを確かめてから、期待値を直す。往復の `assert_eq!` で止まるテストがあれば、`pretty` か `parse` を直す (期待値で合わせない)。

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: 文書を直してコミットする**

- `docs/spec/core-ir.md` に、テストで読み書きする表示の形 (boxed の印 `^`、エフェクトの表の行) と、`eml_core_ir::parse` がそれを読むことを書く。変数の名前が数字で終わるときの読み方も書く
- `docs/implementation/test-changes.md` の「リファクタリング R6」に、Core IR のすべてのスナップショットに `^` とエフェクトの行が入ったこと (種類2)、`core_text` が往復を確かめるようになったこと、`linearity` を消した組み立ての追随 (種類3) を書く

Run: `cargo clippy --all-targets && cargo fmt --check`

```bash
git add -A crates docs
git commit -F - <<'EOF'
Read printed Core IR back with a text parser

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 6: verifier の結合テストを IR のテキストで書く (A5)

**Files:**
- Modify: `crates/eml_core_ir/tests/verify.rs` (958行)
- Modify: `docs/implementation/test-changes.md`

**Interfaces:**
- Consumes: Task 5 の `eml_core_ir::parse`

- [ ] **Step 1: 書き直しの形を決める**

ファイルの先頭の組み立ての関数 (`function`、`arm`、`concat`、`pick` など) を消し、次の2つの関数で書く。

```rust
fn check(text: &str) -> Result<(), String> {
    let program = eml_core_ir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    verify(&program).map_err(|error| error.to_string())
}

fn check_scopes(text: &str) -> Result<(), String> {
    let program = eml_core_ir::parse(text).unwrap_or_else(|error| panic!("{error}"));
    verify_scopes(&program).map_err(|error| error.to_string())
}
```

例: `a_duplicated_string_used_twice_is_accepted` は次になる。

```rust
#[test]
fn a_duplicated_string_used_twice_is_accepted() {
    let text = "fn twice(s0^) {\n  dup s0\n  let t1^ = prim ++(s0, s0)\n  return t1\n}\n";
    assert_eq!(check(text), Ok(()));
}
```

- [ ] **Step 2: すべてのテストを書き直す**

各テストの IR を、今のアリーナの組み立てを読んで同じ IR のテキストにする。変数の番号、boxed かどうか、join point の番号、文字列定数を同じにし、`assert_eq!` の期待値 (受け入れか、誤りの文言) は変えない。`perform_names_an_operation_of_its_effect` のように表の範囲の外の操作を指す IR は、`perform Ask.#5(…)` の形で書く。`a_long_run_of_if_statements_is_verified_in_linear_time` は、テキストを `format!` とループで組む。

テキストで書けない IR (アリーナの形そのものの誤り) があれば、そのテストは Task 3 で作った `verify.rs` の単体テストへ移す。移したテストの名前を報告に書く。

Run: `cargo test -p eml_core_ir --test verify`
Expected: PASS。テストの数は今と同じ (単体テストへ移したものを除く)

- [ ] **Step 3: 記録してコミットする**

`docs/implementation/test-changes.md` の「リファクタリング R6」に、`eml_core_ir/tests/verify.rs` を IR のテキストで書き直したこと (種類3、期待値は同じ)、単体テストへ移したテストがあればその名前を書く。

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`

```bash
git add -A crates docs
git commit -F - <<'EOF'
Write verifier tests as Core IR text

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 7: interp の結合テストを IR のテキストで書く (A5)

**Files:**
- Modify: `crates/eml_interp/tests/closures.rs`、`data.rs`、`run.rs`
- Modify: `crates/eml_test_support/src/lib.rs` (`ir` の使われなくなった関数)
- Modify: `docs/implementation/testing.md`、`docs/implementation/test-changes.md`

**Interfaces:**
- Consumes: Task 5 の `eml_core_ir::parse`、`eml_test_support::execute(program, debug_heap)`

- [ ] **Step 1: すべての手組みの IR を書き直す**

各テストの `Program` を、同じ IR のテキストを `eml_core_ir::parse` で読む形にする。入口の関数は、`entry$main` がなければ最初の関数になるので、今の `main` を最初に書く。実行の結果 (出力と `Result`) の期待値は変えない。`run.rs` の `leaking_program` も同じにする。

例 (`data.rs` の `unpack(false)` に当たる IR):

```
fn main() {
  let s0^ = const "x"
  let d1^ = con #1(s0)
  switch d1 {
    #0 ->
      return ()
    #1(x2^) ->
      let o3 = perform println(x2)
      return o3
  }
}
```

これは形の例で、各テストの IR は今のアリーナの組み立てを読んで、変数、命令、文字列を同じにする。

- [ ] **Step 2: `eml_test_support::ir` を整理する**

`ir::{var, boxed, unboxed, program}` のうち、どのテストからも使われなくなった関数を消す。モジュールが空になれば `ir` ごと消し、`eml_test_support` の先頭の doc コメントの「手書きの Core IR の部品」を消す。

- [ ] **Step 3: テストを流し、記録してコミットする**

Run: `cargo test -p eml_interp && cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

- `docs/implementation/testing.md` に、手で組む Core IR のテストは IR のテキスト (`eml_core_ir::parse`) で書くこと、アリーナの形そのものを確かめるテストだけは `verify.rs` の単体テストで組むことを書く
- `docs/implementation/test-changes.md` の「リファクタリング R6」に、`eml_interp/tests/` を IR のテキストで書き直したこと (種類3、期待値は同じ) と、`eml_test_support::ir` の整理を書く

```bash
git add -A crates docs
git commit -F - <<'EOF'
Write interpreter tests as Core IR text

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 8: `eml_interp` を分ける (A6)

**Files:**
- Modify: `crates/eml_interp/src/lib.rs` (845行)
- Create: `crates/eml_interp/src/machine.rs`、`effects.rs`、`prim.rs`、`io.rs`、`error.rs`
- Modify: `docs/implementation/architecture.md`

**Interfaces:**
- Produces: 公開する型と関数 (`run`、`RunConfig`、`RuntimeError`、`Fault`) は今と同じ名前と形で `lib.rs` から公開する。中の関数は `impl<'p> Machine<'p>` をファイルごとに分けて書く

- [ ] **Step 1: ファイルに分ける**

| ファイル | 中身 (今の `lib.rs` の関数) |
|---|---|
| `lib.rs` | `run`、`RunConfig`、`pub use` |
| `error.rs` | `RuntimeError`、`Fault`、`Display`、`io_reason`、今の `mod tests` のうち誤りの表示のテスト |
| `machine.rs` | `Machine`、`Step`、`Applied`、`ReturnPoint` (今の `Resume`)、`new`、`run`、`step`、`bind`、`call`、`apply_and_continue`、`enter`、`apply`、`take_closure`、`push_frame`、`ret`、`read`、`atom`、`atoms`、`check_leaks` |
| `effects.rs` | `find_handler`、`perform`、`resume` |
| `prim.rs` | `prim`、`take_string` |
| `io.rs` | `io` |

`Machine` の欄は、ほかのファイルの `impl` から使えるよう `pub(crate)` か `pub(super)` にする。`with_debug_heap_sets_the_flag` のテストは `lib.rs` に残す。

- [ ] **Step 2: `Prepared` を消し、`Resume` を改名する**

`call` を次の形にする。フレームの退避 (`push_frame`) は環境のスロットを読むだけで書き換えないので、引数はフレームを積んだ後に読んでよい。

```rust
    /// 呼び出す。`ret` は戻った値を受ける変数と再開する位置で、`None` ならフレームを積まない (末尾呼び出し)。
    pub(crate) fn call(&mut self, call: &Call, ret: Option<ReturnPoint<'p>>) -> Result<Step, Fault> {
        if let Some(ret) = ret {
            self.push_frame(ret)?;
        }
        match call {
            Call::Direct(callee, args) => {
                let args = self.atoms(args)?;
                self.enter(*callee, args);
                Ok(Step::Continue)
            }
            Call::Apply(callee, args) => {
                let callee = self.atom(callee)?;
                let args = self.atoms(args)?;
                self.apply_and_continue(callee, args)
            }
            Call::Handle { effect, body, clauses, ret } => {
                let body = self.atom(body)?;
                let clauses = self.atoms(clauses)?;
                let ret = ret.map(|ret| self.atom(&ret)).transpose()?;
                let frame = Frame::Handler { effect: *effect, clauses, ret, next: Some(self.cont) };
                self.cont = self.heap.alloc(Payload::Frame(frame));
                self.apply_and_continue(body, vec![Value::Unit])
            }
            Call::Perform { effect, op, args } => {
                let args = self.atoms(args)?;
                self.perform(*effect, *op, args)
            }
            Call::Resume { k, arg } => {
                let k = self.atom(k)?;
                let arg = self.atom(arg)?;
                self.resume(k, arg)
            }
        }
    }
```

`enum Prepared` を消す。戻り位置の構造体 `Resume` を `ReturnPoint` に改名し、使う箇所の変数名 (`resume`) も `ret` などにそろえる。継続の再開 (`resume`) と名前が重ならないようにするためである。`Machine::step` などの `Resume { … }` の組み立ても直す。

- [ ] **Step 3: テストを流してコミットする**

Run: `cargo test -p eml_interp && cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS。UI テストの出力は変わらない

`docs/implementation/architecture.md` の「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」に、`eml_interp` のファイルの分け方 (上の表) を書く。

```bash
git add -A crates docs
git commit -F - <<'EOF'
Split the interpreter by concern and drop the prepared call stage

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 9: R6a の残りの片付け

R6a の最終レビューで R6b に回した小さな項目を片付ける。

**Files:**
- Modify: `crates/eml_types/src/check/handle.rs` (`op_clause`)、`crates/eml_types/src/shape.rs` (`instantiate_with_effect_args`)
- Modify: `crates/eml_types/src/carry.rs` (呼び出しの腕の中のコメント)
- Modify: `crates/eml_hir/tests/eval.rs`
- Modify: `docs/implementation/architecture.md` (`call_steps` の項目)、`docs/implementation/status.md` (`eml_hir` の行)

- [ ] **Step 1: `eml_hir` の評価の手順のテストを足す**

`crates/eml_hir/tests/eval.rs` に、今の `PRELUDE` と `steps` を使って足す。

```rust
#[test]
fn a_lambda_beyond_the_arity_is_passed_together() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = h 1 (fn y -> y)"),
        ["eval h", "eval 1", "eval fn y -> y", "arrow 0", "arrow 1"]
    );
}

#[test]
fn an_annotated_value_beyond_the_arity_is_passed_together() {
    assert_eq!(
        steps("t : Int -> Int\nt x = h 1 (x : Int)"),
        ["eval h", "eval 1", "eval x : Int", "arrow 0", "arrow 1"]
    );
}

#[test]
fn a_pipe_into_a_call_beyond_the_arity_applies_before_the_piped_arrow() {
    assert_eq!(
        steps("t : Unit -> Int\nt () = g () |> h 1"),
        ["eval g ()", "eval h", "eval 1", "arrow 0", "arrow 1"]
    );
}
```

ラムダの `h 1 (fn y -> y)` は `h : Int -> (Int -> Int)` の2つ目の引数に `Int` を渡す型の形と合わないが、`lower_clean` は名前解決までしか確かめないので、手順のテストには使える。表示が括弧の有無だけ違えば、括弧だけを合わせる。

Run: `cargo test -p eml_hir --test eval`
Expected: PASS (`call_steps` はすでにこの形で動く。新しいテストは `is_value` の分岐を固定する)

- [ ] **Step 2: 小さな修正**

- `check/handle.rs` の `op_clause` の `match signatures.operations.get(clause.op) { … None => self.table.error }` を、すべての操作に形があるので `&signatures.operations[clause.op]` で直接引く形にする
- `shape.rs` の `instantiate_with_effect_args` の先頭に `debug_assert!(effect_args.len() <= self.rigids.len());` を足す
- `carry.rs` の呼び出しの腕の中の「手順を後ろからたどる。順は eml_hir::call_steps が決め、Core IR の変換も同じ手順を読む」は、ファイルの先頭の doc コメントと重なるので「手順を後ろからたどる」にする
- `docs/implementation/architecture.md` の `call_steps` と `is_value` の項目 (1文が長い) を、評価の順、値の範囲、まとめ方の3つの項目に分ける
- `docs/implementation/status.md` の「各 crate の実装状況」の `eml_hir` の行に、`eval.rs` の `call_steps`、`is_value`、`known_arity` (評価の順と引数のまとめ方) を足す

- [ ] **Step 3: テストを流してコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`

```bash
git add -A crates docs
git commit -F - <<'EOF'
Tidy the R6a leftovers and pin more evaluation steps

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```

---

### Task 10: R6 を閉じる

**Files:**
- Modify: `docs/implementation/status.md`、`docs/future/roadmap.md`、`docs/implementation/architecture.md`
- Delete: `docs/superpowers/specs/2026-10-06-refactor-r6-design.md`、`docs/superpowers/plans/2026-10-06-refactor-r6a.md`、`docs/superpowers/plans/2026-10-06-refactor-r6b.md`

- [ ] **Step 1: spec の残す内容を移す**

`docs/superpowers/specs/2026-10-06-refactor-r6-design.md` を読み、まだ `docs/` に移っていない内容を移す。

- 3章「6b の spec に回すもの」: `docs/implementation/status.md` の「次の作業の注意点」に、段階6b の spec を書くときの入力として項目ごとに写す (HIR の closure の形、`Handle` の `FnRef` とつねにある `ret`、`Atom::Fn`、`Link { next, state }`、`Cont` の状態の欄と `UnifyError` の種類、題名を持つ複数の fix)
- 3章「S2 の設計の材料と、後回しにするもの」: S2 の設計の材料は status.md の「次の作業の注意点」に、後回しのもの (simplify の書き直し、実行時エラーの位置、HIR の source map、n 列の `match`、`let` のラムダの row の多相化、doc comment、記述子) は `docs/future/roadmap.md` の該当する節に、まだ書かれていないものだけを足す
- `docs/implementation/architecture.md` の「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」に、`CExpr` の visitor (`for_each_child`、`for_each_atom`) と `FnBuilder` / `CoreFn::push` の2つの組み立ての口、テキストの IR (`pretty` と `parse`) があることを書く (Task 3 と Task 8 で書いた部分と重ねない)
- 見直しで分かった誤りの記録: `docs/future/roadmap.md` の「再帰する join point」の「生存解析に不動点の計算は要らない」を直す。生存解析 (`liveness::analyze`) は join point の本体を範囲より先に1回だけたどり、`jump` で行き先の `captures` を読むので、自分への `jump` ではまだ空の `captures` を読む。ループ化では、再帰する join point に印を付け、そこだけ不動点を計算する必要がある。simplify の F、B3、B5 も join point が非巡回であることを前提にしている

- [ ] **Step 2: status.md を直す**

- 「R7 で直す項目」の節の先頭の文から、作業用の spec へのリンク (`[R6 の設計](../superpowers/specs/2026-10-06-refactor-r6-design.md)`) を消し、「R6 の見直しで R7 に回した項目は次のとおりである」とする
- リファクタリングの表の R6 の行を「完了」にし、範囲に R6b (Core IR の visitor と `FnBuilder`、`compact` と木の検査、末尾呼び出しの T、テキストの IR と `parse`、`eml_interp` の分割) を足す
- 「R5 で直す項目」と同じ形で「R6 で直す項目」の節を足し、R6a と R6b で済んだことを数文でまとめる
- 「完了した作業」の表に R6 の行を足す
- 「次の作業の注意点」のうち、R6 で直したもの (不具合4の末尾呼び出し、`(f 1) (g ())` の評価の順) が残っていれば消す

- [ ] **Step 3: 作業用の文書を消す**

```bash
git rm docs/superpowers/specs/2026-10-06-refactor-r6-design.md docs/superpowers/plans/2026-10-06-refactor-r6a.md docs/superpowers/plans/2026-10-06-refactor-r6b.md
```

`grep -rn "superpowers/" docs crates` が何も返さないことを確かめる (消した文書を指すリンクやコメントが残っていない)。

- [ ] **Step 4: 全体を確かめてコミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS

spec の「完了の条件」の R6b の項目を確かめる (消す前の spec を `git show HEAD:docs/superpowers/specs/2026-10-06-refactor-r6-design.md` で読む)。

- `grep -rn "CExprId(0)\|CExprId(1)" crates/eml_core_ir/tests crates/eml_interp/tests` が何も返さない
- `grep -rn "Prepared\|linearity" crates/eml_interp/src crates/eml_core_ir/src` が何も返さない
- `grep -rn "vars.pop" crates/eml_core_ir/src/translate` が何も返さない

```bash
git add -A docs
git commit -F - <<'EOF'
Close refactor R6 and remove its working design and plans

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01UdzCttvymdYwHt9daX3bTB
EOF
```
