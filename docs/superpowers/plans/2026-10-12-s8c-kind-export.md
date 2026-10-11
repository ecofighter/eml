# S8c 書き出す型の線形性 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `eml_types` が書き出す型に矢印の線形性と決まらなかった型変数の Kind を持たせ、instance ごとの Kind の評価 (`kind_values`、`uniform_kind`) と型の線形性 (`linearity`) を公開し、単相化をそれに合わせ、`unr` と `lin` が出会わないことを debug ビルドで確かめる。

**Architecture:** まず表現だけを変え、書き出す線形性はすべて `Unr` にしておく (Task 1)。次に型検査を SCC ごとの「解いてから書き出す」順に組み替える (Task 2)。宣言のスキームの上の評価 (`kind_values` など) を足し (Task 3)、単相化がそれを使うようにする (Task 4)。ここまでは既存の出力が1バイトも変わらない。Task 5 で本当の書き出し (要約、`Var`、`OpVar` の Kind) に切り替え、Task 6 で debug ビルドの検査を入れ、Task 7 で文書を移す。

**Tech Stack:** Rust (edition 2024)、insta、eml の UI テスト。

**Spec:** `docs/superpowers/specs/2026-10-12-s8c-kind-export-design.md` (main の 11759b8)。計画も Task 1 の前にコミットする。実装する人は、各タスクの前に spec の該当する節を読む。spec が規範で、計画とずれたら spec に従い、ずれを報告する。コードの場所と今の定義は、補助として `/private/tmp/claude-501/-Users-arakaki-Projects-eml/12dc2b67-583d-46c7-bf7d-fab5bb80c0fa/scratchpad/s8c/codemap.md` (11759b8 の行番号) にまとめてある。行番号は作業が進むとずれるので、名前で探す。

| 見出し | 中身 | 既存のテストの期待値 |
|---|---|---|
| Task 1 | `Lin`、`TypeKind` の新しい形、`TypeStore` の列 (`linearity`、Kind 変数、相対)、`Substitution::new`/`partial`、`lin_compatible`。書き出す線形性はすべて `Unr` | spec の「機械的な変更」と、「期待値の変更」の2件 |
| Task 2 | `check_module` を SCC ごとの「検査、解く、書き出す」に組み替える | 変わらない |
| Task 3 | スキームの上の評価: 定数への畳み込み、`kind_values`、`uniform_kind`、`needs_substitution` | 変わらない |
| Task 4 | 単相化が Task 3 の評価を使う。名前の表示 (`%1`、`%m`) | 変わらない |
| Task 5 | 本当の書き出し: 要約と `joins`、`Var`、`OpVar` の Kind、`Shape::export` の2つの使い方、線形性のダンプ、scaling テスト | spec の「名前の変わるテストはない見込み」を確かめる。変わったら止めて報告する |
| Task 6 | 食い違いがないことの検査 (参照、操作、捕獲) | 変わらない |
| Task 7 | 文書、ロードマップ、`CLAUDE.md`、spec と計画の削除 | 変わらない |

## Global Constraints

- 各タスクの終わりに、次のすべてが通る。途中のタスクでも、ワークスペース全体を壊したままコミットしない
  - `cargo test` (ワークスペース全体。debug ビルドなので、Task 6 の検査も全 UI テストと `bench/` で走る)
  - `cargo clippy --all-targets` が警告を出さない。`cargo fmt --check` が差分を出さない
  - `cargo clippy -p eml_cli --no-default-features --features types` と `--features core`
  - `cargo test -p eml_cli --test integration citations`
- UI テストの出力 (`crates/eml_cli/tests/snapshots/`) と診断の文字列は、どのタスクでも変えない。`bench/` の `RunStats` (`crates/eml_interp/tests/bench.rs`) も変えない
- 既存のテストの期待値は、上の表の「既存のテストの期待値」の列の範囲でだけ変える。スナップショットは、変わった中身を読み、spec の規則を満たすことを確かめてから受け入れる。新しいスナップショットも、中身を読んで spec の規則に照らしてから受け入れる
- テストの成否は変えない (spec の「合否の変更」は「ない」)。必要になったら作業を止めて報告する (CLAUDE.md の Testing)
- 名前 (各タスクで共通に使う)
  - `eml_types::Lin { Unr, Lin, Poly, Var(u32) }` (Task 1)
  - `TypeKind::Fn { param, lin: Lin, effects, tail, ret }`、`TypeKind::Flexible(Lin)`、`TypeKind::OpVar(String, Lin)` (Task 1)
  - `TypeStore::flexible(&mut self, kind: Lin) -> TypeId`、`TypeStore::linearity(&self, ty) -> Lin`、`TypeStore::lin_compatible(&self, a, b) -> bool`、`TypeStore::contains_kind_vars(&self, ty) -> bool` (Task 1)
  - `Substitution::new(vars: impl IntoIterator<Item = (String, TypeId)>, kinds: Vec<Lin>)`、`Substitution::partial(vars)` (Task 1)
  - `TypedProgram::kind_values(&self, decl: ValueItem, args: &[TypeId], store: &TypeStore) -> Vec<Lin>`、`TypedProgram::uniform_kind(&self, function: FunctionId, position: usize) -> Lin`、`TypedProgram::needs_substitution(&self, decl: ValueItem) -> bool` (Task 3)
  - `TypeStore::display_linear_bounded(&self, ty, names, limit) -> Option<String>` (Task 4、instance の名前)
  - `eml_types::dump_linear(program, typed) -> String` (Task 5、線形性を表示するダンプ)
  - `DeclType.joins: Vec<Vec<u32>>` (crate の中、Task 5)
- `Lin` の結びは `Lin` > `Poly` > `Unr` で、`Var` は結びに入れない (評価した後の値だけを結ぶ)。`Lin` に `Ord` を derive しない (宣言の順と結びの順が違うため)。結びは `Lin::join(self, other) -> Lin` の1か所に置く
- 日本語のコメントと文書は、書く前に `yomiyasu:yomiyasu` のスキルを呼び、その規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く。spec の内容がまだ `docs/spec/` に移っていない間は、`docs/superpowers/specs/2026-10-12-s8c-kind-export-design.md` の節を引く (`citations.rs` は `docs/superpowers` を見ないので引いてよい)。Task 7 で `docs/spec/` に移すときに引き直す。コメントに計画のタスクの番号を書かない
- コミットメッセージは英語で、既存のコミットの形 (命令形の1行目、本文に理由) に合わせる。テストの期待値を変えたコミットは、本文に変更の種類 (期待値の変更、機械的な追随) と理由を書く。各コミットの末尾に、会話で指定された `Co-Authored-By` と `Claude-Session` の行を付ける
- このリポジトリの `git diff` は外部のツールを使う。普通の差分は `git diff --no-ext-diff` で見る

## Review Focus

spec が求めるが、どのタスクのテストも直接は確かめない形のうち、使う人が最初に踏みそうなもの。各行の確かめは、対応するタスクに入れてある。

- 多相再帰の中で、`File` を捕まえたクロージャが型変数を通って渡る形 (spec の「instance での評価」の `f` の例)。`f@[Int]` の `k` が `unr` にならず、再帰の呼び出しが `f@[Int %m -> Int]` になり、捕獲の検査を通る (Task 3 の `kind_values` のテスト、Task 5 と Task 6 の `eml_core_ir` のテスト、`eml run` の UI 的な確かめ)
- 型変数のない関数 (`main`) の本体にある handler の節。本体の型は写さず、`OpVar` の Kind をそのまま読んでも `linearity` が止まらない (Task 5 のテスト `a_clause_in_main_keeps_its_operation_variable_kinds`)
- 線形性だけが違う具体化で、instance が分かれ、名前が `id@[Int %1 -> Int]` と `id@[Int -> Int]` になる。一方で、`lin` と `unr` のクロージャで呼ぶ `map` は1つの instance のまま (Task 5 の `eml_core_ir` のテスト)
- 大きな SCC での要約の時間 (spec の「メモリと時間」の3つの形)。release ビルドで `scaling::` の `--ignored` テストが通る (Task 5)
- 診断と UI の出力が1バイトも変わらない (全タスクの Global Constraints。Task 2 と Task 5 で特に崩れやすい)

---

### Task 1: 表現を変える (線形性はすべて `Unr`)

spec の「書き出す型の形」の `Lin` と `TypeKind`、「型の線形性」、「代入と単相化」の `Substitution` の形、「食い違いがないことの検査」の比べ方 (`lin_compatible`)。書き出しと代入はまだ線形性を使わない。書き出す矢印はすべて `Unr`、`Flexible` と `OpVar` も `Unr` にするので、instance の鍵も名前も今と同じになる。

**Files:**
- Modify: `crates/eml_types/src/store.rs` (`Lin`、`TypeKind`、`TypeStore` の列、`intern`、`substitute`、`Substitution`、`flexible`、`linearity`、`lin_compatible`、表示、単体テスト)
- Modify: `crates/eml_types/src/lib.rs` (`pub use store::Lin`。`pub use ty::{Linearity, Multiplicity}` を消す)
- Modify: `crates/eml_types/src/ty.rs` (`Linearity`、`Multiplicity` を `pub(crate)` にする)
- Modify: `crates/eml_types/src/table/export.rs`、`crates/eml_types/src/shape.rs` (`export_ty`)、`crates/eml_types/src/check/mod.rs` (`check_main`)、`crates/eml_types/src/uniform.rs`、`crates/eml_types/src/resolve.rs` (パターン)
- Modify: `crates/eml_core_ir/src/translate/types.rs`、`crates/eml_core_ir/src/translate/instances.rs` (パターン、`flexible(Lin::Unr)`、`Substitution::new(.., Vec::new())`)
- Modify: `crates/eml_types/tests/instantiations.rs` (期待値の変更)、`crates/eml_core_ir/tests/externs.rs` (機械的な変更)

**Interfaces:**
- Consumes: なし
- Produces: Global Constraints の Task 1 の名前すべて。`TypeStore` は `Clone + Send + Sync` のまま (内部の可変性を持たない)

- [ ] **Step 1: `Lin` と `TypeKind` の新しい形を書く**

`store.rs` に `Lin` を足し、`TypeKind` を変える。`Lin` の doc コメントは spec の「`Lin`」の節に合わせる。

```rust
/// 書き出した型の矢印の線形性と、決まらなかった型変数の Kind
/// (docs/superpowers/specs/2026-10-12-s8c-kind-export-design.md の「書き出す型の形」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lin {
    Unr,
    Lin,
    /// 呼び出し側が決める。…(spec の doc コメントをそのまま)
    Poly,
    /// 宣言の Kind 変数。…
    Var(u32),
}

impl Lin {
    /// 結び。`Lin` > `Poly` > `Unr` で、`Var` は評価した後の値だけを結ぶので受けない。
    pub fn join(self, other: Lin) -> Lin {
        match (self, other) {
            (Lin::Var(_), _) | (_, Lin::Var(_)) => unreachable!("an unevaluated Kind variable is joined"),
            (Lin::Lin, _) | (_, Lin::Lin) => Lin::Lin,
            (Lin::Poly, _) | (_, Lin::Poly) => Lin::Poly,
            (Lin::Unr, Lin::Unr) => Lin::Unr,
        }
    }
}
```

`TypeKind::Fn` に `lin: Lin` を足し (欄の順は `param, lin, effects, tail, ret`)、`Flexible` を `Flexible(Lin)`、`OpVar(String)` を `OpVar(String, Lin)` にする。`TypeKind::Fn` の doc コメントの「矢印の線形性は持たない…」を、書き出す矢印は線形性を持ち、宣言の中では `Var` もありうる、と改める。`for_each_child` は `lin: _` を名前で受ける。

- [ ] **Step 2: `TypeStore` の列を足す**

`TypeStore` に、`errors` と `vars` と同じく型ごとの列を3つ足し、`intern` で子から求める。

- `kind_vars: Vec<bool>`: `Fn { lin: Var(_) }` か `Flexible(Var(_))` をどこかに含む
- `relative: Vec<bool>`: 値が `Rigid` か `Var` に依存する。`Fn` は自分の `lin` だけで値が決まるので、`Fn { lin: Var(_) }` のときだけ立てる (子を見ない)。`Con` と `Tuple` は、効く子 (`Con` は `DataKind.params` が真の位置) のどれかが立っていれば立てる。`Rigid` と `Flexible(Var(_))` は立てる。`OpVar` は Kind が定数なので立てない
- `linearity: Vec<Lin>`: spec の「型の線形性」の表のとおり。`relative` のときは `Poly` を入れる

`TypeStore::new` で `crate::data::data_kinds(program)` を呼び、`data_kinds: ItemMap<TypeDef, DataKind>` を持たせる (`Context::new` も同じ関数を呼ぶが、二重に計算しても害はない)。`flexible` の欄 (1つの `Flexible`) は消し、`flexible(kind: Lin)` は毎回 `intern(TypeKind::Flexible(kind))` を返す。

```rust
/// 型の値の線形性 (spec の「型の線形性」)。宣言に対して相対的な型 (`Rigid` か `Var` に値が依存する型) は意味のある値を
/// 持たないので、debug ビルドでは止める。
pub fn linearity(&self, ty: TypeId) -> Lin {
    debug_assert!(!self.relative[ty.index()], "the linearity of a declaration-relative type is read");
    self.linearity[ty.index()]
}

/// 型に宣言の Kind 変数 (`Var`) が残っているか。
pub fn contains_kind_vars(&self, ty: TypeId) -> bool { self.kind_vars[ty.index()] }
```

- [ ] **Step 3: `Substitution` を新しい形にする**

```rust
pub struct Substitution {
    vars: HashMap<String, TypeId>,
    /// 宣言の Kind 変数の値。`None` は `partial` で、`Var` をそのまま残す。
    kinds: Option<Vec<Lin>>,
    done: HashMap<TypeId, TypeId>,
}
impl Substitution {
    pub fn new(vars: impl IntoIterator<Item = (String, TypeId)>, kinds: Vec<Lin>) -> Substitution
    /// 型引数だけを置き換え、Kind 変数は残す。宣言の型の一部の型引数だけを置き換えるテストのため。
    pub fn partial(vars: impl IntoIterator<Item = (String, TypeId)>) -> Substitution
}
```

`substitute` の早い戻りを `!vars && !kind_vars` にし、`Fn { lin: Var(i) }` と `Flexible(Var(i))` を `kinds[i]` に置き換える (`kinds` が `Some` で `i` が範囲の外なら `debug_assert` で止める。spec の「代入と単相化」)。`OpVar(_, k)` は `Flexible(k)` にする。`Flexible(Unr | Lin | Poly) | Error` は今と同じく来ない (`vars` も `kind_vars` も立たない)。

- [ ] **Step 4: `lin_compatible` を書く**

spec の「食い違いがないことの検査」の比べ方。

```rust
/// 2つの型の対応する線形性が、一方が `Unr`、他方が `Lin` になっていないか。`Poly` はどちらとも食い違わない。片方が
/// `Flexible(k)` か `OpVar(_, k)` で他方が別の形なら、`k` と他方の値を比べる。`Rigid` はどの型とも合う。形が一致しない
/// 所 (誤りの跡) は比べない。
pub fn lin_compatible(&self, a: TypeId, b: TypeId) -> bool
```

- 2つの値の比べ方 `fn meets(a: Lin, b: Lin) -> bool` は、`{Unr, Lin}` の組だけを偽にする。`Var` は `Poly` と同じに扱う (宣言の型と比べる操作の検査のため)
- 同じ形 (`Con` と `Con`、`Tuple` と `Tuple`、`Fn` と `Fn`) なら子を並べてたどる。`Fn` は `lin` を比べ、引数、ラベルの型引数、戻り値をたどる (ラベルの数が違えば、そこは比べない)
- 片方が `Flexible(k)` か `OpVar(_, k)` なら、他方の `linearity` の値 (相対なら `Poly` とみなす。`linearity` の debug の止め方を通らないよう、列を直接読む内部の関数を使う) と `k` を比べる
- 片方が `Rigid` なら真。ほかの形の食い違いは真 (比べない)
- 同じ組を2回たどらないよう、`(TypeId, TypeId)` の組で覚える

- [ ] **Step 5: 書き出しを `Unr` で埋める**

- `table/export.rs`: `TyShape::Fn` は `lin: Lin::Unr`、`TyShape::Var(_)` は `types.flexible(Lin::Unr)`、per-operation の rigid は `OpVar(name, Lin::Unr)`
- `shape.rs` の `export_ty`: `Fn` は `lin: Lin::Unr`
- `check/mod.rs` の `check_main`: 期待する型に `lin: Lin::Unr`
- `uniform.rs`、`resolve.rs`、`translate/types.rs`、`externs.rs` のテスト: パターンを新しい形にする
- `translate/instances.rs`: `self.store.flexible()` を `self.store.flexible(Lin::Unr)` に、`Substitution::new(pairs)` を `Substitution::new(pairs, Vec::new())` にする。`TypeKind::Flexible` のパターンは `Flexible(_)`
- `externs.rs` のテストの `Substitution::new` は `Substitution::partial` にする

この時点では、どこにも `Var` も `Poly` も `Lin` も現れないので、鍵、名前、ダンプは今と同じになる。

- [ ] **Step 6: `store.rs` の単体テストを直し、新しいテストを書く**

機械的な変更: 9か所の `TypeKind::Fn` に `lin: Lin::Unr`、4か所の `Substitution::new` に `Vec::new()` を足す。期待値の変更: `operation_variables_become_flexible_and_unknown_names_stay` は `OpVar("a", Lin::Poly)` を代入して `Flexible(Lin::Poly)` になることを確かめる形にする。

新しいテスト (同じ `mod tests`):

```rust
#[test]
fn linearity_joins_the_effective_arguments_and_fields() {
    // `File` は Lin、`List File` は Lin、`List Int` は Unr、効かない型引数は見ない、タプルは結び、
    // `Fn` は自分の lin、`Flexible(k)` と `OpVar(_, k)` は k
}

#[test]
fn a_type_that_depends_on_a_rigid_or_a_kind_variable_is_relative() {
    // `Rigid("a")`、`Fn { lin: Var(0) }`、`Flexible(Var(1))`、`List a` は相対。`Fn { lin: Unr, param: Rigid }` と
    // `OpVar("a", Unr)` は相対でない。相対かどうかは内部の列を読む `#[cfg(test)]` の関数で見る
}

#[test]
fn substitution_evaluates_kind_variables() {
    // `Fn { lin: Var(0) }` と `Flexible(Var(1))` が [Lin, Poly] で `Fn { lin: Lin }` と `Flexible(Poly)` になる。
    // `partial` は `Var` を残す
}

#[test]
fn lin_compatible_rejects_only_unr_against_lin() {
    // 表: (Unr, Lin) 偽、(Lin, Unr) 偽、(Poly, 何でも) 真、(Unr, Unr) 真、入れ子の `Fn` の引数の食い違い 偽、
    // `Flexible(Unr)` と `File` 偽、`Flexible(Poly)` と `File` 真、`OpVar("a", Unr)` と `Int` 真、`Rigid` と何でも 真
}
```

`List` は `crate::test_program("")` の Prelude、`File` は `program.extern_type(ExternType::File)` で引く (`kind/mod.rs` のテストの書き方)。テストの本体は、表を配列にして回す形で書く。

- [ ] **Step 7: 期待値の変更と確かめ**

`crates/eml_types/tests/instantiations.rs` の `TypeKind::OpVar("a".to_string())` を `TypeKind::OpVar("a".to_string(), Lin::Unr)` にする (spec の「期待値の変更」。`a` は引数の型に現れるので、最終の値も `Unr`)。

Run: `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、features の clippy 2つ。Expected: 全部通り、上の2件のほかに期待値の変わったスナップショットがない。

- [ ] **Step 8: Commit**

`Represent arrow linearity in exported types` のような1行目で、本文に「書き出す線形性はまだすべて Unr」「機械的な変更 (store.rs のテスト、externs.rs) と期待値の変更 2件 (OpVar の Kind)」を書く。

---

### Task 2: 型検査を SCC ごとの「検査、解く、書き出す」に組み替える

spec の「型検査の順」(「新しい順」と「推論の途中で書き出す型」の `check_main` まで)。書き出す値はまだ Task 1 のまま (`Unr`) なので、結果は1バイトも変わらない。Task 5 が、ここで作る「解いた後に書き出す」場所に本当の値を入れる。

**Files:**
- Modify: `crates/eml_types/src/check/mod.rs` (`check_module`、`check_body` を2つに分ける、`check_method_kinds` の引数)
- Modify: `crates/eml_types/src/table/mod.rs` (`take_problem`)

**Interfaces:**
- Consumes: Task 1 の `TypeStore`
- Produces (crate の中):
  - `fn check_body(..) -> Option<(PendingBody<'c>, Vec<Diagnostic>)>`: 段1。推論用の表、`BodyTyping`、`own` (`Own` か、`own.lin`/`own.mult`/`own_rows`)、具体化の記録、`reliable` を持つ
  - `fn export_body(pending: PendingBody<'_>, types: &mut TypeStore) -> BodyTypes`: 書き出し。今の `check_body` の 434〜500 行の書き出し (row の分類を含む) をここに移す。Task 5 がこの関数に解と要約を渡す引数を足す
  - `Table::take_problem(&mut self, instances: Vec<Instance>, own: OwnVars) -> KindProblem`: `linearity`、`multiplicity`、`carries` を `std::mem::take` で取り出し、表 (形) は残す

- [ ] **Step 1: 組み替える前に、今の出力を控える**

Run: `cargo test` (全部通ることを確かめる)。組み替えの後に同じテストが通れば、出力が同じことの確かめになる (UI と `eml_types` のダンプはスナップショットなので、1バイトでも変われば落ちる)。

- [ ] **Step 2: `check_body` を段1と書き出しに分ける**

今の `check_body` の、`usage::constrain` と `carry::constrain` までを段1に残し、`Exporter` を使う書き出しを `export_body` に移す。`into_problem` の代わりに `take_problem` で Kind の制約を取り出し、表は `PendingBody` に残す。`declaration_problem` は今の `into_problem` のままでよい。

- [ ] **Step 3: `check_module` の順を変える**

spec の「新しい順」のとおり。

1. instance の検査、シグネチャ、`declaration_schemes`、`check_main` の後に、等式のない関数に空のスキームを入れる処理を移す。条件は「シグネチャを持ち、`program.body(id)` が `None`」で、extern の関数のスキームは上書きしない (`or_default`)
2. `scc::components(program)` の SCC ごとに、メンバーの `check_body` を呼び (`None` のメンバーは飛ばす)、`take_problem` した問題で `solve_scc` を解き、スキームを入れ、違反を集め、各メンバーを `export_body` で書き出して `bodies` に入れる。`PendingBody` (表) はそこで捨てる
3. `check_method_kinds` には、問題の代わりに「本体を検査した関数」の `HashSet<FunctionId>` を渡す (今は `problems.get(id)` を「あるか」にしか使っていない)
4. 残り (違反の報告、`typed_program`、一様な位置、網羅性) は今のまま

`TypeId` の登録の順は変わるが、後の段階は `TypeId` の値に依存しないので出力は変わらない。

- [ ] **Step 4: 確かめる**

Run: `cargo test`、clippy、fmt、features の clippy。Expected: 全部通り、スナップショットの差分がない。Run: `cargo test --release -p eml_types --test integration scaling:: -- --ignored`。Expected: 今と同じく通る (表を SCC の分だけ持つことで遅くならない)。

- [ ] **Step 5: Commit**

`Check, solve and export each call-graph SCC in turn` のような1行目で、本文に「本体の型を Kind を解いた後に書き出すための組み替え。出力は変わらない (機械的)」を書く。

---

### Task 3: スキームの上の評価

spec の「instance での評価」(`val` の不動点と `Unr` への固定)、「`Flexible` と `OpVar` の Kind」の `uniform_kind`、「代入と単相化」の写すかどうかの判定。書き出しはまだ変えないので、ここで足す関数は Task 4 と Task 5 が使い始める。

**Files:**
- Create: `crates/eml_types/src/kind/eval.rs` (スキームの上の評価)
- Modify: `crates/eml_types/src/kind/mod.rs` (`mod eval`)、`crates/eml_types/src/lib.rs` (`TypedProgram` のメソッド、`DeclType.joins`)
- Create: `crates/eml_types/tests/kind_values.rs`、Modify: `crates/eml_types/tests/main.rs` (モジュールの宣言)

**Interfaces:**
- Consumes: Task 1 の `TypeStore::linearity`、`Lin::join`
- Produces:
  - `pub(crate) struct SchemeFacts { pub fixed_unr: Vec<bool>, pub forced_lin: Vec<bool> }` と `pub(crate) fn scheme_facts(scheme: &KindScheme, lin_vars: usize) -> SchemeFacts` (`kind/eval.rs`)。`fixed_unr[v]` はスキームの辺を上にたどって定数の `Unr` に届くこと、`forced_lin[v]` は下にたどって定数の `Lin` に届くこと。どちらも推移的にたどる
  - `pub(crate) fn values(scheme: &KindScheme, lin_vars: usize, base: impl Fn(usize) -> Lin, facts: &SchemeFacts) -> Vec<Lin>`: spec の `val` の不動点。`base(v)` は型変数なら `linearity(args[i])`、矢印なら `Poly`
  - `DeclType.joins: Vec<Vec<u32>>` (この Task では、どの宣言でも空)
  - `TypedProgram::kind_values`、`TypedProgram::uniform_kind`、`TypedProgram::needs_substitution` (Global Constraints の形)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/kind_values.rs` を作り、`tests/main.rs` に `mod kind_values;` を足す。補助の関数:

```rust
use eml_hir::{ExprKind, ValueItem};
use eml_test_support::check;
use eml_types::Lin;

/// `caller` の本体で最初に `callee` を参照した具体化の型引数で、`callee` の `kind_values` を求める。
fn values_at(text: &str, callee: &str, caller: &str) -> Vec<Lin> {
    let checked = check(text);
    assert!(checked.diagnostics.is_empty(), "{:?}", checked.diagnostics);
    // caller と callee の FunctionId を名前で引き (入口のモジュールの関数)、`typed.bodies[caller].instantiations` から
    // `decl == ValueItem::Function(callee)` の最初の具体化を取り、`typed.kind_values(decl, &inst.args, &typed.types)` を返す
}
```

テスト (spec の「新しいテスト」の `kind_values` と `uniform_kind`。期待値は spec の規則から決めたもの):

```rust
#[test]
fn an_arrow_fixed_to_unr_by_its_use_is_unr() {
    // twice : (Int -> Int) -> Int -> Int、twice f x = f (f x)。`f` の矢印は Unr に固定される
    // values_at(text, "twice", "main") の `f` の矢印の位置が Unr
}

#[test]
fn an_arrow_the_callee_does_not_fix_is_poly() {
    // apply : (a -> b) -> a -> b、apply f x = f x。`f` の矢印は Poly、`a` と `b` は Int なら Unr
}

#[test]
fn a_type_variable_takes_the_linearity_of_its_argument() {
    // keep : a -> (Unit -> a)、keep x = fn () -> x。main が File と Int で呼ぶ。`μ_a` は File で Lin、Int で Unr
}

#[test]
fn a_type_variable_above_a_poly_arrow_is_poly() {
    // spec の「instance での評価」の f の例 (mu_poly)。f@[Int] で `μ_a` が Poly になる (`linearity(Int)` は Unr だが、
    // 下の `g` の矢印の Poly が伝わる)
}

#[test]
fn an_arrow_below_a_type_variable_fixed_to_unr_is_unr() {
    // スキームに m ≤ μ ≤ Unr が残る形 (μ を2回使う関数に、矢印を型変数として渡す SCC)。m が Unr になる
}

#[test]
fn variables_in_one_cycle_have_one_value() {
    // スキームで μ と m が輪になる形。kind_values の μ と m の値が等しい
}

#[test]
fn uniform_kind_is_unr_only_when_the_scheme_fixes_it() {
    // depth (x を2回使う、一様な位置) は Unr、1回使う一様な関数は Poly、別の関数を通して Unr に固定される型変数は Unr
}

#[test]
fn needs_substitution_follows_type_variables_and_unfixed_kind_variables() {
    // 型変数を持つ関数は真、型変数も Kind 変数も持たない main は偽、型変数がなく矢印の Kind 変数が畳まれない関数は真
}
```

各テストの本文には、spec の規則のどれを確かめるかを日本語のコメントで書き、`.em` のテキストは既存の `eml_types` のテスト (`tests/effects.rs` など) と同じく文字列で書く。どの位置の値を見るかは、`Shape` の番号 (外側から、型変数の `μ` と矢印の `m`) で数え、テストのコメントに番号の対応を書く。

Run: `cargo test -p eml_types --test integration kind_values::`。Expected: `kind_values` などが無くてコンパイルできない。

- [ ] **Step 2: `kind/eval.rs` を書く**

- `scheme_facts`: `KindScheme.lin` の `(Bound, Bound)` の組から、変数ごとの上向きと下向きの辺、定数の上限と下限を作り、`fixed_unr` (上向きの閉包に `Const(Unr)` の上限) と `forced_lin` (下向きの閉包に `Const(Lin)` の下限) を求める。宣言ごとに `O(lin_vars + 制約の数)` にする (変数ごとにたどり直さず、上限は下から上へ、下限は上から下へ伝える固定点にする)
- `values`: spec の `val`。`fixed_unr[v]` なら `Unr`。そうでなければ `base(v)`、`v` の定数の下限、下にある変数の `val` の結び。輪のある制約なので、ワークリストの固定点で求める (値は `Unr` < `Poly` < `Lin` の向きに上がるだけなので止まる)

- [ ] **Step 3: `TypedProgram` のメソッドを書く**

```rust
impl TypedProgram {
    /// 宣言の `Var(i)` の、型引数 `args` (`Shape::rigids` の順) の instance での値。i < lin_vars は宣言自身の変数、
    /// それ以降は `joins` の結び (spec の「instance での評価」)。
    pub fn kind_values(&self, decl: ValueItem, args: &[TypeId], store: &TypeStore) -> Vec<Lin>
    /// 関数の一様な位置 `position` の型変数の Kind。スキームで `Unr` に固定されていれば `Unr`、そうでなければ `Poly`。
    pub fn uniform_kind(&self, function: FunctionId, position: usize) -> Lin
    /// 単相化で宣言の本体の型を写す必要があるか。型変数を持つか、定数に畳まれない宣言自身の線形性の変数を持つ。
    pub fn needs_substitution(&self, decl: ValueItem) -> bool
}
```

`base` は、`Shape::rigids[i].1` (型変数 `i` の `μ` の番号) の位置に `store.linearity(args[i])` を、それ以外の番号に `Lin::Poly` を置く。`args[i]` が `Flexible(k)` なら `k` になる (Task 1 の `linearity`)。`DeclType` に `joins` を足し、`typed_program` では空にしておく。

- [ ] **Step 4: テストを通す**

Run: `cargo test -p eml_types --test integration kind_values::`。Expected: PASS。値が spec の規則と違えば、規則の読み違いか実装の誤りかを確かめ、spec と食い違う期待値には変えない。

- [ ] **Step 5: 全体を確かめる**

Run: `cargo test`、clippy、fmt、features の clippy。Expected: 既存のスナップショットは変わらない。

- [ ] **Step 6: Commit**

`Evaluate declaration Kind variables at an instance` のような1行目。

---

### Task 4: 単相化が評価を使う

spec の「代入と単相化」(`Substitution::new` に `kind_values`、一様な位置と entry と生成器の鍵、`field_types`、写すかどうか、`Var` が残らない検査) と「instance の名前」。書き出す型はまだすべて `Unr` なので、鍵と名前は変わらない。

**Files:**
- Modify: `crates/eml_core_ir/src/translate/instances.rs`
- Modify: `crates/eml_types/src/store.rs` (`display_linear_bounded` と表示の型)

**Interfaces:**
- Consumes: Task 1 の `Substitution::new`、`flexible`、`contains_kind_vars`。Task 3 の `kind_values`、`uniform_kind`、`needs_substitution`
- Produces: `TypeStore::display_linear_bounded(&self, ty: TypeId, names: &DisplayNames, limit: usize) -> Option<String>` (`display_bounded` と同じ形で、矢印の線形性を `%1`、`%m` で書き、`Unr` は書かない。`Flexible` は Kind によらず `_`)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/src/store.rs` の単体テストに:

```rust
#[test]
fn the_linear_display_marks_lin_and_poly_arrows() {
    // Fn { lin: Lin, Int, Int } は "Int %1 -> Int"、Poly は "Int %m -> Int"、Unr は "Int -> Int"。
    // 入れ子の引数の矢印にも付く ("(Int %1 -> Int) -> Int")。Flexible(Lin) は "_"。row は今の表示と同じ
}
```

Run: `cargo test -p eml_types --lib store::`。Expected: FAIL (関数がない)。

- [ ] **Step 2: 表示を書く**

`TypeDisplay` に表示の種類 (診断と、線形性付き) を持たせ、線形性付きのときだけ `Fn` の矢印の前に ` %1` か ` %m` を入れる (Haskell の LinearTypes の `a %1 -> b`)。`Var(i)` は名前には現れないが、表示するなら `%m{i}` にする (Task 5 のダンプが使う)。診断の表示は1文字も変えない。

- [ ] **Step 3: `instances.rs` を変える**

- `function_body`: `Substitution::new(names.zip(args), self.typed.kind_values(ValueItem::Function(function), &found.args, &self.store))`
- `add_uniform`: 一様な位置を `self.store.flexible(self.typed.uniform_kind(function, position))` にする
- `add_generated`: 一様な位置を `self.store.flexible(Lin::Poly)` にする
- `collect` の entry: 位置ごとに `flexible(uniform_kind(entry, i))`
- 写すかどうか (今の `(!names.is_empty()).then(..)`): `self.typed.needs_substitution(ValueItem::Function(function))` にし、コメントを spec の「代入と単相化」に合わせる
- 写した後の `debug_assert` (型変数が残らない) に `!self.store.contains_kind_vars(ty)` を足す
- `field_types`: 構成子ごとに `kind_values(ValueItem::Constructor(c), &generated.args, &self.store)` を求め、構成子ごとの `Substitution::new` で代入する
- `instance_name`: `display_bounded` を `display_linear_bounded` にする

- [ ] **Step 4: 確かめる**

Run: `cargo test -p eml_types --lib store::`、`cargo test -p eml_core_ir`、`cargo test`、clippy、fmt、features の clippy。Expected: 全部通り、instance の名前を含むスナップショットは変わらない (書き出す線形性がまだすべて `Unr` のため)。

- [ ] **Step 5: Commit**

`Monomorphize with evaluated Kind variables` のような1行目。

---

### Task 5: 本当の書き出し

spec の「宣言の Kind 変数の番号」、「推論の途中で書き出す型」の `Exporter` と `Shape::export` の2つの使い方、「値の規則」の「書き出すときの規則」、「`Flexible` と `OpVar` の Kind」の `OpVar` の記録、「メモリと時間」。ここで初めて、書き出す型に `Lin`、`Poly`、`Var` が現れる。

**Files:**
- Modify: `crates/eml_types/src/kind/solve.rs` (`solve_scc` が書き出しに要るもの (メンバーの番号のずれ、グラフ、最小解) を返す)
- Create: `crates/eml_types/src/kind/summary.rs` (要約の動的計画法と `joins`)
- Modify: `crates/eml_types/src/table/export.rs` (2つの使い方)、`crates/eml_types/src/table/mod.rs` (`RigidInfo` の記録、`ty_var_linearity`)、`crates/eml_types/src/shape.rs` (`export` の2つの使い方、`clause_kinds` を per-operation の rigid を作る前に呼ぶ、`kind_names` は診断の使い方)、`crates/eml_types/src/check/mod.rs` (`export_body` に解を渡す、`typed_program` に `joins` とスキームを渡す)、`crates/eml_types/src/check/report.rs`、`usage.rs`、`check/constraints.rs` (診断の使い方)
- Modify: `crates/eml_types/src/dump.rs`、`crates/eml_types/src/lib.rs` (`pub use dump::dump_linear`)
- Create: `crates/eml_types/tests/kind_export.rs`、Modify: `crates/eml_types/tests/main.rs`、`crates/eml_types/tests/scaling.rs`、`crates/eml_types/tests/kind_values.rs` (`joins` の値のテストを足す)
- Modify: `crates/eml_core_ir/tests/instances.rs` (新しいテスト)

**Interfaces:**
- Consumes: Task 2 の `PendingBody` と `export_body`、Task 3 の `scheme_facts` と `values`、Task 4 の表示
- Produces:
  - `Exporter::diagnostic(table, types)` と `Exporter::final_(table, types, kinds: &mut BodyKinds<'_>)` (名前は実装で決めてよい。診断の使い方は `ArrowLin::Known(l)` を `l` に、`ArrowLin::Var(_)` を `Poly` に、決まらなかった型変数を `Flexible(Poly)` にし、`OpVar` は記録した Kind にする)
  - `BodyKinds`: メンバーの表の Kind 変数から `Lin` (定数か `Var`) への写像。spec の「書き出すときの規則」を実装し、`joins` を初めて現れた順に番号付けする
  - `Shape::export(&self, types, scheme: Option<&KindScheme>) -> TypeId` (`None` は `check_main` の使い方)
  - `pub fn dump_linear(program: &Program, typed: &TypedProgram) -> String`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/kind_export.rs` (`tests/main.rs` に足す) に、`dump_linear` の出力を inline スナップショットで見るテストを書く。`dump_linear` の形は `dump` と同じで、宣言と局所の型を線形性付きで表示し (`%1`、`%m`、`Var(i)` は `%m{i}`、`Flexible` の Kind は `_`、`_%1`、`_%m`、`_%m{i}`)、`joins` があれば宣言の下に `  joins: %m2 = %m0 | %m1` の形の行を足す。

```rust
#[test]
fn a_lambda_that_captures_nothing_is_unr() {
    // main の中の let f = fn x -> x + 1 の f は `Int -> Int` (Unr)
}

#[test]
fn a_lambda_that_captures_a_file_is_lin() {
    // let g = fn () -> Fs.close h の g は `Unit %1 -> <IO> Unit`
}

#[test]
fn a_body_variable_below_own_variables_joins_them() {
    // pair : (a -> b) -> (c -> d) -> ...、f と g を両方捕まえるラムダの矢印が `%m{k}` で、joins の行が出る
}

#[test]
fn a_body_variable_in_an_own_variable_component_takes_its_var() {
    // spec の「書き出すときの規則」の f eat x n の例。fn () -> x の矢印が `%m{μ_a の番号}`。注釈 (g : Int -> Int) も
    // g と同じ値
}

#[test]
fn an_own_variable_fixed_by_constants_is_folded() {
    // twice の f の矢印は宣言の型でも本体の型でも Unr (`%` が付かない)
}

#[test]
fn joins_are_numbered_in_export_order() {
    // 2つの異なる joins の集合が、式の ID の順に %m{lin_vars}、%m{lin_vars+1} になる
}

#[test]
fn a_clause_in_main_keeps_its_operation_variable_kinds() {
    // effect Put where put : a -> Unit と mk : Unit -> Box a を main の handle で扱う。節の x は `a` (OpVar、Unr) と
    // 結果だけの OpVar (Poly)。ダンプでは OpVar の Kind を `a` / `a%m` のように出す (OpVar は名前の後に `%m` を付ける)
}

#[test]
fn an_unresolved_variable_takes_the_least_value() {
    // tests/ui/run/files/file_in_return_clause_on_abort.em の形。使われない return x の x は `_` (Unr)
}
```

`crates/eml_types/tests/kind_values.rs` に、`joins` の値のテストを足す (`pair` の例で、`f` と `g` に `lin` と `unr` のクロージャを渡したときの `Var(lin_vars)` が `Lin`)。

`crates/eml_core_ir/tests/instances.rs` に (spec の「新しいテスト」の `eml_core_ir` の項):

```rust
#[test]
fn instances_that_differ_only_in_linearity_have_different_names() {
    // id k (once の k) と id inc。instances_of(text, "id") == ["id@[Int %1 -> <..> Int]", "id@[Int -> Int]"] の形
    // (row の表示は実際の出力を読んで決める)
}

#[test]
fn a_function_called_with_lin_and_unr_closures_has_one_instance() {
    // map を File を捕まえたラムダと何も捕まえないラムダで呼ぶ。instances_of(text, "map") が1つ
}

#[test]
fn a_type_variable_above_a_poly_arrow_makes_the_recursive_call_poly() {
    // spec の「instance での評価」の f の例。instances_of(text, "f") == ["f@[Int]", "f@[Int %m -> <IO> Int]"]
}

#[test]
fn a_result_only_operation_variable_is_a_poly_key() {
    // 節の中で、結果だけの操作ごとの型変数の値を一様でない関数 id に渡す。鍵が Flexible(Poly) で名前は id@[_]
    // (鍵の Kind は名前に出ないので、instance の型を store から読んで確かめる補助を使う)
}
```

Run: 該当のテスト。Expected: FAIL。

- [ ] **Step 2: `solve_scc` から書き出しに要るものを返す**

`Solution` (か、`check_module` だけが使う別の構造体) に、メンバーごとの番号のずれ (`merge` の offsets)、`Graph<Linearity>` (成分、定数の境界、下向きの辺、メンバーの自分の変数)、`solve_with_causes` の最小解の値を持たせる。`declaration_schemes` と `check_method_kinds` の呼び出しは今の形 (スキームと違反) だけを使う。

- [ ] **Step 3: 要約を書く (`kind/summary.rs`)**

spec の「メモリと時間」と「書き出すときの規則」。メンバーごとに:

- そのメンバーの自分の変数を含む成分 (`kept_components`) を求め、自分の変数の畳み込み (Task 3 の `scheme_facts` を、そのメンバーの新しいスキームに当てる) を持つ
- 書き出す本体の変数 (表の変数 `v` は、合わせた番号 `v + offset`) の成分 `c` について
  - `c` が自分の変数を含むなら、その中で番号のいちばん小さい変数 (畳まれていればその定数)
  - そうでなければ、成分の DAG を下にたどる動的計画法 (成分ごとに「定数の `Lin` に届くか」と「届く自分の成分の集合」を覚える。自分の成分で止まる。メンバーごとに覚えを作り直す)
  - `lin_vars == 0` のメンバーは、たどらずに最小解の値を使う
- 集合は spec のとおり整える (`Lin` に畳まれた変数を含めば `Lin`、`Unr` に畳まれた変数は除く、空なら `Unr`、1つならその `Var(i)`、2つ以上なら `joins` に番号を付けて `Var(lin_vars + k)`)。同じ集合は同じ番号。番号は初めて現れた順

- [ ] **Step 4: `Exporter` と `Shape::export` を2つの使い方にする**

- 段1の中の書き出し (`report.rs` の `show`、`usage.rs` の `OmittedReturn`、`constraints.rs` の E2006 と E2009、`check_derived_fields`、`Table::show`) は診断の使い方にする
- `export_body` は解と要約を受け取り、最終の使い方で書き出す。決まらなかった型変数は、その型変数の `μ` (`ty_var_linearity` で引く) を同じ規則で書き出した値の `Flexible` にする
- per-operation の rigid に「引数の型に現れる」を記録する: `instantiate_with_effect_args` で `clause_kinds` を rigid を作る前に呼び、`fresh_operation_rigid(name, linearity, in_params)` で `RigidInfo` に持たせる。書き出しは `in_params` なら `OpVar(name, Unr)`、そうでなければ `OpVar(name, Poly)`
- `Shape::export(types, Some(scheme))` は畳まれた変数を定数に、残りを `Var(i)` にする。`typed_program` はこれを使い、関数の `joins` を `DeclType` に入れる。`check_main` は `None` を使う。`kind_names` の表示用の `Fn` は `Poly` で作る
- Task 3 の `kind_values` は、`i ≥ lin_vars` の番号に `joins[k]` の値の結びを返すようにする

- [ ] **Step 5: `dump_linear` を書く**

`dump.rs` の `dump` と同じ並びで、Task 4 の線形性付きの表示を使う。`OpVar` は名前の後に Kind を付ける (`Unr` は付けない、`Poly` は `%m`)。`joins` の行を足す。

- [ ] **Step 6: テストを通し、全体を確かめる**

Run: `cargo test -p eml_types`、`cargo test -p eml_core_ir`、`cargo test`、clippy、fmt、features の clippy。Expected: 新しいテストが通る。既存のスナップショットは変わらない。instance の名前を含む既存のテスト (`crates/eml_core_ir/tests/` の `@[`) が変わったら、spec の「名前の変わるテストはない見込み」と食い違うので、作業を止めて報告する。

新しいスナップショットは、spec の規則で1行ずつ確かめてから受け入れる (どの変数がどの規則で値を得たかを、テストのコメントに書く)。

- [ ] **Step 7: scaling テストを足す**

`crates/eml_types/tests/scaling.rs` に、spec の「メモリと時間」の3つの形の生成器とテストを足す (今のテストと同じく `#[ignore = "release ビルドで時間を測る"]`、`assert_linear`)。

1. 関数を引数に取る関数の輪: `f_i : (Int -> Int) -> Int -> Int`、`f_i g n = if n == 0 then g n else f_{i+1} g (n - 1)` の n 個の輪
2. 各メンバーが局所のラムダ (`fn u -> x`) を持つ関数の輪
3. 宣言自身の変数を持たない多くのメンバーが、長い矢印の鎖 (`q_n ≤ … ≤ q_1`) の上を参照する SCC。`q_i : Unit -> (Int -> Int)` の連なりを作り、`f_i n = (q_1 ()) 0 + …` の形のメンバーを置く (生成器のコメントに、どの形がどの制約を作るかを書く)

Run: `cargo test --release -p eml_types --test integration scaling:: -- --ignored`。Expected: すべて通る。

- [ ] **Step 8: Commit**

`Export solved arrow linearity and Kind summaries` のような1行目で、本文に「既存の出力は変わらない」と新しいテストを書く。

---

### Task 6: 食い違いがないことの検査

spec の「食い違いがないことの検査」。3つの検査を debug ビルドだけで行う (`debug_assert` か `cfg!(debug_assertions)`)。

**Files:**
- Modify: `crates/eml_core_ir/src/translate/instances.rs` (参照の検査、`perform` の検査、節の検査)
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (捕獲の検査: ラムダと、知っている関数と構成子の部分適用)
- Modify: `crates/eml_core_ir/tests/instances.rs` (検査を通るテスト)

**Interfaces:**
- Consumes: Task 1 の `lin_compatible` と `linearity`、Task 3 の `kind_values`、Task 5 の書き出し
- Produces: なし (debug ビルドの検査だけ)

- [ ] **Step 1: 検査を書く前に、検査が通るはずのテストを足す**

`crates/eml_core_ir/tests/instances.rs` に、Task 5 の `f` の例を `eml_test_support::run` で実行して `2` を出すテスト (`a_closure_with_a_file_passed_as_a_type_variable_runs`) を足す。検査は debug ビルドの `core_until` と `run` の中で走るので、検査を入れた後にこのテストが通ることが、検査が正しいプログラムを拒まないことの確かめになる。

- [ ] **Step 2: 参照の検査**

`collect` の待ち行列を回し終えた後 (`order` の前) に、各関数の instance の中の参照 (`targets` の `FoundTarget::Function(j)` で、`found[j]` が関数の instance のもの) について、参照の式の型 (写した表があればそれ、なければ `typed.bodies[f].exprs`) と `found[j].signature` を `lin_compatible` で比べ、偽なら `debug_assert!` で止める。メッセージには関数と instance の名前を入れる。生成器と extern は比べない。

- [ ] **Step 3: 操作の検査**

- `perform`: 各関数の instance の具体化のうち `decl` が `ValueItem::Operation(op)` のものについて、`kind_values(Operation(op), args)` と `Substitution::new(操作の型変数の名前 zip args, 値)` で操作の宣言の型を代入し、参照の式の型と比べる
- 節: handle の節ごとに、引数のパターンの型と `k` の型を、操作の宣言の型 (代入しない。`Rigid` はどの型とも合い、`Var` は `Poly` として比べる。`lin_compatible` がそう比べる) と比べる。節の引数のパターンと `k` は `eml_hir` の `OpClause` (`args()`、`k()`) から引く

- [ ] **Step 4: 捕獲の検査**

- ラムダ (`expr.rs` のラムダの組み立て): ラムダの型の最初の矢印の `lin` が `Unr` なら、`body.closure_captures(closure)` の各局所変数の型の `linearity` がすべて `Unr` であることを `debug_assert!` で確かめる
- 知っている関数と構成子の部分適用 (`saturate` の `args.len() < arity` の枝): `split_arrows(callee_ty, args.len())` の残りの型の矢印の `lin` が `Unr` なら、捕まえる引数の型がすべて `Unr`。extern と操作の部分適用は spec の「確かめる所」に入らないので確かめない
- spec の「確かめない所」(状態のある `k v`、実行時の部分適用、節と handle の本体のクロージャ、継続を包む関数) には入れない

- [ ] **Step 5: 全体を確かめる**

Run: `cargo test` (全 UI テストと `bench/` が debug ビルドで検査を通る)、clippy、fmt、features の clippy。Expected: 全部通る。検査が正しいプログラムで止まったら、spec の規則か書き出しの誤りなので、作業を止めて止まった所と型を報告する (検査を弱めて通さない)。

検査が実際に誤りを見つけることを、手元で一度確かめる: Task 3 の `values` の型変数の結びから下の変数の値を外す (spec の「instance での評価」の「`μ_a` の値を `linearity(Int)` と定数だけから求める」誤り) と、Step 1 のテストが捕獲の検査で止まることを見てから元に戻す。この確かめはコミットしない。

- [ ] **Step 6: Commit**

`Check in debug builds that unr and lin never meet` のような1行目。

---

### Task 7: 文書

spec の「ドキュメントの変更」のすべてと、spec と計画の削除。

**Files:**
- Modify: `docs/spec/types.md`、`docs/spec/core-ir.md`、`docs/implementation/architecture.md`、`docs/future/roadmap.md`、`CLAUDE.md`、コードの doc コメント (spec の「ドキュメントの変更」の一覧)
- Modify: Task 1〜6 のコメントのうち、`docs/superpowers/specs/2026-10-12-s8c-kind-export-design.md` を引くもの (`docs/spec/` の節に引き直す)
- Delete: `docs/superpowers/specs/2026-10-12-s8c-kind-export-design.md`、`docs/superpowers/plans/2026-10-12-s8c-kind-export.md`

- [ ] **Step 1: `docs/spec/types.md`**

spec の「ドキュメントの変更」の `types.md` の4項目。新しい節「Kind の書き出し」(「row の具体化の記録」の後) に、書き出す時点、値の規則と `joins`、`Flexible` と `OpVar` の Kind、instance での評価 (`val`、`Unr` への固定)、`μ` の instance での値を固定に使わない理由を、spec の文を整えて移す。

- [ ] **Step 2: `docs/spec/core-ir.md`**

「型の Repr」の「Kind は残さない」の補足、「変換の規則」の鍵と名前と代入、「食い違いがないことの検査」(translate の debug ビルドの検査として、3つの検査と確かめない所)。

- [ ] **Step 3: `docs/implementation/architecture.md` の「`eml_types` の内部」**

spec の6項目 (段の順、`Exporter` と `Shape::export` の2つの使い方、`Instantiation.args`、`Lin` と crate の中の型、ID の同一性、query の注記)。

- [ ] **Step 4: `docs/future/roadmap.md` と `CLAUDE.md`**

roadmap の S8 の節: 表を S8a〜S8d の4行にし、S8c を終えたことと S8d の中身、V2 の定義を Repr の互換と同じ形に改め、V2 の向きの未決の項目と「S8a からの申し送り」を閉じる。`CLAUDE.md` (英語): 書き出す型が線形性を持つこと (`Lin`、`Flexible(Lin)`、`OpVar(_, Lin)`)、SCC ごとに解いてから書き出すこと、`kind_values` / `uniform_kind` / `linearity`、名前の `%1` / `%m`、debug ビルドの3つの検査を、今の Architecture の箇条の該当する所に足す。

- [ ] **Step 5: コメントの引き先を直し、spec と計画を消す**

`grep -rn "2026-10-12-s8c" crates docs` で引き先を探し、`docs/spec/` の節に直す。spec と計画のファイルを消す。

- [ ] **Step 6: 確かめる**

Run: `cargo test -p eml_cli --test integration citations`、`cargo test`、clippy、fmt。Expected: 全部通る。

- [ ] **Step 7: Commit**

`Document exported linearity and delete the S8c design and plan` のような1行目。
