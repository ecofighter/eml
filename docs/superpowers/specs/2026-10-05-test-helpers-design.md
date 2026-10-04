# テスト補助コードの統一の設計

位置づけ: 作業用の設計文書。この作業を終えたら、この文書は削除する。決めた内容は `crates/eml_test_support/`、各 crate の `tests/common/`、`docs/implementation/testing.md` に入る。

## 目的と範囲

テストとテストの文書の整理の、サブプロジェクト2である ([status.md](../../implementation/status.md) の「次の作業の注意点」の「テストの整理の残り」)。各 crate が別々に書いている同じ処理を、`eml_test_support` と各 crate の `tests/common/` に寄せる。

### 直すもの

- 段階の表示の後ろに `---` と診断を足す処理を、`eml_syntax`、`eml_hir`、`eml_types` の `tests/common` と、`eml_syntax` の `lexer.rs` と `parser.rs` の `dump` が別々に書いている
- 診断のないソースの結果を取り出す処理を、`eml_syntax/tests/ast.rs` の `source` と `eml_hir/tests/structure.rs` の `module` が別々に書いている
- HIR の診断を位置の順に並べる処理を、`eml_hir` の `tests/common` と `tests/effects.rs` の `errors` が別々に書いている
- 手書きの Core IR を組む部品 (`VarInfo` の組み立て、`var(n)`、`Program` の組み立て) を、`eml_core_ir/tests/verify.rs`、`eml_interp/tests/closures.rs`、`eml_interp/tests/run.rs` がほぼ同じ形で別々に書いている

### 範囲の外

- スナップショットの診断の形式。今は ariadne の表示 (`lexer.rs` と `parser.rs` の `dump`)、バイトの位置 (`lexer.rs` の `diags`)、`short`、`full` の4つがある。そろえると期待値が変わる (種類2) ので、この作業では扱わない
- `verify.rs` と `closures.rs` の `function`。前者は式の列から、後者は `Step` の列から組み、組み方が違うので別々に残す
- 1つのファイルだけが使う補助 (`kinds`、`value`、`first_equation`、`main_with` など)

### ユーザーと合意済みの決定

- テストの変更は種類3だけにする。スナップショットの文字列と `assert` の値は1文字も変えない
- 複数の crate で使う部品は `eml_test_support` に置き、crate の表示の関数は各 crate の `tests/common/mod.rs` に残す
- 前回のレビューで後に回した3件の小さな指摘を、この作業の文書の更新に含める (4章)

## 1. eml_test_support に足すもの

| 関数 | feature | 中身 |
|---|---|---|
| `with_diagnostics(dump: String, diagnostics: &str) -> String` | なし | `diagnostics` が空でなければ、`dump` の後ろに `---\n` と `diagnostics` を足して返す。空なら `dump` をそのまま返す |
| `short_text(files: &SourceFiles, diagnostics: &[Diagnostic]) -> String` | なし | `short` の各行の後ろに改行を付けてつないだ文字列。`full` の文字列版に当たる |
| `parse_clean(text: &str) -> Parsed` | なし | `parse` して、診断が1件もないことを確かめてから返す。診断があれば `short` の行を表示して panic する |
| `lower_clean(text: &str) -> Lowered` | `hir` | `lower` について同じことをする |
| `ir::var(n: u32) -> Atom` | `core` | `Atom::Var(VarId(n))` |
| `ir::boxed(name: &str) -> VarInfo`、`ir::unboxed(name: &str) -> VarInfo` | `core` | `linearity` が `Unr` で、`boxed` がそれぞれ `true` と `false` の `VarInfo` |
| `ir::program(functions: Vec<CoreFn>, entry: u32, strings: &[&str]) -> Program` | `core` | `effects` が空の `Program`。エフェクトを持つ `Program` は `Program { effects, ..ir::program(...) }` と書く |

- 診断が空かどうかの判定は、今の各所の判定と同じ結果になる。`short` は診断がなければ空の列を、`full` と `render` は空の文字列を返すためである
- `parse_clean` と `lower_clean` は、エラーだけでなく警告も含めて「診断が1件もない」ことを確かめる。今の `source` と `module` と同じ条件である

## 2. 各 crate の tests/common

| crate | 変更 |
|---|---|
| `eml_syntax` | `shape` を `with_diagnostics` と `short_text` で組む。`diagnostics`、`helps`、`item_kinds`、`lines` は変えない |
| `eml_hir` | 位置の順に並べた `Lowered` を返す非公開の関数を置き、`lower_text` と、新しい `diagnostics` (`effects.rs` の `errors` を移して名前を変えたもの) がそれを使う。`eml_syntax` の同じ名前の関数と同じく、`short` の行の列を返す |
| `eml_types` | `check_text` を `with_diagnostics` と `full` で組む |
| `eml_core_ir` | 変えない |

- 関数を2つ以上持つ `tests/common/mod.rs` には、`eml_syntax` と同じく `#![allow(dead_code)]` を付ける。各テストファイルは一部の関数しか使わないためである

## 3. 移す順とテスト

次の順に、1段ずつコミットする。各段の終わりに `cargo test` と `cargo clippy --all-targets` を通す。

1. `eml_test_support` に1章の関数を足す。テストを先に書き、失敗を見てから実装する
2. `eml_syntax`: `common::shape`、`lexer.rs` と `parser.rs` の `dump`、`ast.rs` の `source`
3. `eml_hir`: `common` (並べ替えと `diagnostics`)、`effects.rs` の `errors`、`structure.rs` の `module`
4. `eml_types`: `common::check_text`
5. Core IR の部品: `verify.rs` の `string`、`int`、`var`、`Program` の組み立て、`closures.rs` の `VarInfo`、`var`、`Program` の組み立て、`run.rs` の `var` のクロージャと `Program` の組み立て
6. 文書 (4章)

### 新しい関数のテスト

`crates/eml_test_support/tests/support.rs` に足す。既存のテストは変えない。

- `with_diagnostics`: 空の診断なら `dump` がそのまま返る。診断があれば `---\n` と診断が続く
- `short_text`: 各行が改行で終わる。診断がなければ空の文字列になる
- `parse_clean` と `lower_clean`: 誤りのないソースなら結果を返す。誤りのあるソースなら panic する (`#[should_panic]`)
- `ir`: `program`、`boxed`、`unboxed`、`var` で組んだ小さなプログラムが `eml_core_ir::verify` を通り、`effects` が空で `strings` が渡したとおりになる

### 種類3であることの確かめ方

2〜5の各段で、次の2つを確かめる。

- `INSTA_UPDATE=no cargo test` を流し、`.snap.new` と `.pending-snap` のファイルができない。値が変わればテストが失敗し、書き換えられない
- `git diff -U0` の変更行に、インラインスナップショット (`@"`) と `assert_eq!` / `assert!` の期待値の行が含まれない。変わるのは組み立ての行だけである。例外は、`ast.rs` の `source` と `structure.rs` の `module` の「診断がない」ことの `assert!` で、これは期待値ではなく前提の確認なので、同じ条件のまま `parse_clean` と `lower_clean` に移る
- `.snap.new` と `.pending-snap` は `.gitignore` にあるので、`git status` ではなく `find` で確かめる

## 4. 文書

- `docs/implementation/testing.md` の「crate の中の置き方」に、複数の crate で使う部品は `eml_test_support` に置くことを書き、`eml_test_support` が持つ関数の一覧に1章の関数を足す
- `CLAUDE.md` の `eml_test_support` の行に、`short_text`、`with_diagnostics`、`parse_clean` / `lower_clean`、`ir` を足す
- `docs/implementation/status.md` の「テストの整理の残り」から、テスト補助コードの統一を除く。テスト本体の整理だけを残す
- 種類3の変更なので、`test-changes.md` には書かない。コミットメッセージで足りる

前回のレビューで後に回した指摘を、次のとおり直す。

- testing.md の置き場所の表の3行目に、生成した大きなソースを足す。`eml_interp/tests/run.rs` の長い文の列と、`eml_core_ir/tests/verify.rs` の `a_long_run_of_if_statements_is_verified_in_linear_time` が使っている
- testing.md の地図の `eml_types` の `effects.rs` に「線形な継続 (E3001)」を、`eml_runtime` の `heap.rs` に「`take` と `take_or_copy` による複製」を足す
- `test-changes.md` の「書き方」に、R1 までの項目は種類の番号を持たないが、履歴なので書き足さないことを書く
