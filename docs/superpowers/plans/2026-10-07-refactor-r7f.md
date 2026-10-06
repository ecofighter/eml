# リファクタリング R7f 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** R7 を締めくくる。作業用の設計文書の残す価値のある内容を `docs/` に移して参照を張り替え、R7e-2 で後回しにした軽微な修正を済ませ、`architecture.md` を簡素にし、作業用の文書を削除して R7 を完了にする。

**Architecture:** 先に作業用の設計文書の内容を `docs/` に移し、コードと文書のすべての参照を移した先に張り替える。次に後回しの修正 (古いコメント、内部の誤りの文言、B2 の重複した計算) をする。続けて `architecture.md` から、コードを読めば分かる細部を減らす。最後に `status.md` とロードマップを R7 の完了に合わせ、作業用の設計文書と R7 の計画を削除する。

**Tech Stack:** Rust (edition 2024)、`cargo test`、Markdown の文書。

**Spec:** `docs/superpowers/specs/2026-10-06-refactor-r7-design.md` の 7.3 (文書の更新) と 7.5 (R7f 後始末)。R7e-2 の最終の見直しで後回しにした項目。

## Global Constraints

- 互換性は気にしない。後方互換のための分岐や別名は作らない (CLAUDE.md)
- コードのコメントと `docs/` は日本語で書く。コメントは「なぜ」を書き、`docs/` の規則を指すときはパスを書く。日本語を書くときは `yomiyasu:yomiyasu` スキルの規則に従う (CLAUDE.md)
- `docs/` の既存の書き方 (英単語の前後の半角空白、箇条書き) に合わせる。文末のコロンは書かない
- 設計をテストに合わせて曲げない。テストの変更は種類1 (振る舞い)、種類2 (内部表現のスナップショット)、種類3 (期待値が同じ機械的な追随) に分け、種類1と種類2は `docs/implementation/test-changes.md` の「リファクタリング R7f」の節に記録する (docs/implementation/testing.md)
- 期待値を変えてよいのは、Task 2 に挙げたもの (内部の誤りの文言の1件、種類1) だけである。ほかの期待値が変わったら、受け入れずに DONE_WITH_CONCERNS で報告する
- Task 1 の後は、コードと `docs/` (`docs/superpowers/` を除く) のどこも `docs/superpowers/specs/2026-10-06-refactor-r7-design.md` を指さない。Task 2 以降で新しく指さない
- `architecture.md` の見出しのうち、コードのコメントが指すもの (「`eml_types` の内部」、「`eml_hir` の内部」、「`eml_hir` で行う脱糖と検査」) と、「`Table::export`」の規則 (後の段階は Kind を読まない) の説明は残す
- 各タスクの終わりに `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` を通す
- コミットのメッセージは英語で書き、末尾に次の2行を付ける
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y
  ```
- `git diff` は外部ツールを使う設定なので、スクリプトで差分を見るときは `git diff --no-ext-diff` を使う
- 作業中にファイルを一時的に戻すときは `git checkout <file>` を使わない。戻す前にファイルを写しておき、写しから戻す

## spec からの補い

- 張り替えの対応 (作業用の設計文書の章 → 移した先)。移した先に該当する説明がなければ、その文書に1文足してから指す
  - 1.4 (Prelude と session、`main` は入口のモジュールから探す) → `docs/implementation/architecture.md` の「CLI と lib API」
  - 1.5 と 3.2 (名前の解決の順、重複の3つの規則) → `docs/spec/modules.md` の「名前の解決」と「名前空間」
  - 3.1 と 3.2 (`ItemTree`、`DefMap`) → `docs/implementation/architecture.md` の「`eml_hir` の内部」
  - 4.2 (intrinsic の表) → `docs/implementation/architecture.md` の「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」
  - 4.4 (Prelude の本体、`|>` と `<|`) → `docs/spec/declarations.md` の標準の演算子の表
  - 4.5、4.6、6.3 (入口から届く関数、`Prelude.` の名前、`Bool` のタグ、平らな `Switch`、1つの葉からだけ届く枝) → `docs/spec/core-ir.md`
  - 5.1〜5.3 (`TypedProgram`、`DeclType`、由来の `Span`、`lower` の入口の引数) → `docs/implementation/architecture.md` の「`eml_types` の内部」と「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」
  - 6.1 (`saturate`) → `docs/implementation/architecture.md` の「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」
  - 「方式」(item を単位にしたプログラム全体のパイプライン) と、各章の「採らなかった形」 → Task 1 で `architecture.md` に足す節
- `docs/implementation/test-changes.md` の過去の記録が作業用の設計文書の章を指している箇所も、上の対応で移した先に張り替える。記録の中身は変えない
- 後回しの修正のうち、R7e-2 の最終の見直しで直したもの (verifier のテスト、`fields_allowed` の順、F の説明、B3 の文、`default` の所有、`FnBuilder::finish`) は済んでいる。この計画で直すのは、残った3つ (古いコメント2か所、内部の誤りの文言、B2 の重複した計算) である
- R7f の計画 (この文書) は、最終の見直しが済んだ後にコントローラが削除する。Task 4 は、作業用の設計文書と R7a〜R7e-2 の計画を削除する

## Review Focus

- コードのコメントと文書から、作業用の設計文書への参照が1つも残らない (`grep -rn 2026-10-06-refactor-r7-design crates docs --exclude-dir=superpowers` が空。Task 1 で確かめ、Task 4 で設計文書を削除した後にも確かめる)
- 張り替えた参照の先に、指している内容が本当にある (Task 1 の各張り替えで、移した先の文書のその節を読んで確かめる)
- `architecture.md` を簡素にしても、コードのコメントが指す見出しと規則が残る (Task 3 で `grep -rn "architecture.md の「" crates docs` の指す先を確かめる)
- `architecture.md` から消した説明のうち、決定とその理由 (なぜそうしたか) は失わない。コードの名前の言い換えだけを消す (Task 3 の確かめ方)
- 内部の誤りの文言を「case」にそろえても、インタプリタの振る舞いは変わらない (Task 2 で `eml_interp` のテストを流す)

---

## ファイルの構成

| ファイル | 変更 | タスク |
|---|---|---|
| `docs/implementation/architecture.md` | 「プログラム全体の構成」の節を足す (方式と採らなかった形)。張り替えの先に足りない説明を足す | 1 |
| コードのコメント (`crates/` の23ファイル)、`docs/implementation/status.md`、`test-changes.md` | 作業用の設計文書への参照を張り替える | 1 |
| `crates/eml_core_ir/tests/simplify.rs`、`crates/eml_core_ir/src/translate/mod.rs` | 「残りの枝の join point」を指す古いコメント | 2 |
| `crates/eml_interp/src/machine.rs`、`crates/eml_interp/tests/data.rs` | 内部の誤りの文言を「case」にそろえる | 2 |
| `crates/eml_core_ir/src/simplify.rs` | B2 で値の行き先を1回だけ求める | 2 |
| `docs/implementation/architecture.md` | コードの細部の説明を減らす | 3 |
| `docs/implementation/status.md`、`docs/future/roadmap.md` | R7 の完了 | 4 |
| `docs/superpowers/specs/2026-10-06-refactor-r7-design.md`、`docs/superpowers/plans/2026-10-0[67]-refactor-r7{a,b-1,b-2,b-3,c,d,e-1,e-2}.md` | 削除 | 4 |

---

### Task 1: 作業用の設計文書の内容を移し、参照を張り替える

**Files:**
- Modify: `docs/implementation/architecture.md`
- Modify: 作業用の設計文書を指すコードのコメント (下の Step 1 の一覧)
- Modify: `docs/implementation/status.md`、`docs/implementation/test-changes.md`
- 必要なら Modify: `docs/spec/modules.md`、`docs/spec/declarations.md`、`docs/spec/core-ir.md` (張り替えの先に足りない1文を足すときだけ)

**Interfaces:**
- Produces: `architecture.md` の新しい節「プログラム全体の構成」。Task 3 はこの節を簡素化の対象にしない (短く書いてあるため)

- [ ] **Step 1: 参照の一覧を作る**

Run: `grep -rn "2026-10-06-refactor-r7-design" crates docs --exclude-dir=superpowers`
Expected: 約40件 (コード23ファイル、`status.md` 3件、`test-changes.md` 8件)。各行が指す章の番号 (「の 4.5」など) を控える。章の番号がない参照 (`crates/eml_hir/src/program.rs:1`、`crates/eml_core_ir/tests/translate.rs:661` など) は、その行の内容から章を決める。

- [ ] **Step 2: `architecture.md` に「プログラム全体の構成」を足す**

書く前に `yomiyasu:yomiyasu` スキルを読み込む。`architecture.md` の「全体の方針」の後に、次の内容の節を足す。作業用の設計文書の「方式」の節と、各章の採らなかった形を、今のコードに合う形で短くまとめる (全体で20行程度)。

- 処理系は item を単位にしたプログラム全体のパイプラインである。名前解決は `ItemTree` (ファイルごとの宣言の要約)、`DefMap` (モジュールごとのスコープ表)、item ごとの変換の3段で、ID (`ItemId`) はプログラム全体で一意である。型検査は宣言、本体、SCC の粒度で動き、モジュールの境目を使わない
- モジュールごとに検査して依存先のインタフェースを借りる方式は採らない。相互再帰がモジュールをまたぐと SCC もまたぐので import の循環を許せなくなること、型検査が自分と依存先の2系統の表を引くことになること、インタフェースとしての出力を別に設計する必要があることが理由である。item を単位にすれば、import の循環を許すかどうかは `DefMap` の作り方だけの問題になる
- 採らなかった形 (理由を1文ずつ)
  - salsa を今入れること (段階をクエリの形にしてあるので後で載せ替えられる。LSP を作るまでは手間が大きい)
  - モジュールごとに Core IR を作ってリンクすること (インタプリタでは得るものがない)
  - HIR の位置を今 source map に移すこと (利点はクエリ化してから効く)
  - プログラム全体の item を1つのアリーナに置くこと (モジュールの変換が共有のアリーナを書き換え、段階が純粋な関数でなくなる)
  - 型検査の出力の `Type` から名前をなくすこと (S2 で同じ名前の別の型の表示を決めるときに扱う。今は `DeclType::ty` が書き出した `Type` を持つ)
  - どの item も Core IR の関数にして、`simplify` の規則で命令に戻し、使わない関数を取り除くパスで消すこと (変換が満ちた呼び出しを命令にすでにしているので、飽和の場合分けを `saturate` にまとめれば足りる)
  - `Bool` のタグをインタプリタまで運ぶこと (タグは Prelude の宣言の順で決まるので、定数 `FALSE` と `TRUE` をテストで照らし合わせれば足りる)

- [ ] **Step 3: 参照を張り替える**

「spec からの補い」の対応表に従って、Step 1 の各参照を移した先に張り替える。コメントの形は `(docs/spec/core-ir.md)`、`(docs/implementation/architecture.md の「`eml_hir` の内部」)` のように、既存のコメントの書き方に合わせる。

- 張り替える前に、移した先の文書のその節を読み、指している内容 (たとえば「1つの葉からだけ届く枝は join point にしない」) が書いてあることを確かめる。なければ、その節に1文足してから指す
- `docs/implementation/status.md` の、作業用の設計文書を指す3か所のうち、R7 の行 (101行目付近) は Task 4 で書き直すので、ここでは参照だけを外す。「R7 を終えるときに … 参照を … 張り替える」の項目 (174行目付近) は、この Step で済むので消す。同じ名前の別の型の表示の項目 (203行目付近) は、`Type` から名前をなくす案を項目の中に1文で書き、参照を外す
- `docs/implementation/test-changes.md` の8か所は、記録の中身を変えずに、章の参照だけを移した先にする

Run: `grep -rn "2026-10-06-refactor-r7-design" crates docs --exclude-dir=superpowers`
Expected: 何も出ない。

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS (コメントだけの変更なので、期待値は変わらない)。

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/architecture.md`
Expected: 英単語の前後の空白と箇条書きの比率の指摘だけ (既存の書き方に合わせて残す)。文末のコロンがあれば直す。

- [ ] **Step 4: コミットする**

```bash
git add crates docs/implementation docs/spec
git commit -m "Move the R7 design into the docs and repoint every reference to it

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```

---

### Task 2: 後回しにした軽微な修正

**Files:**
- Modify: `crates/eml_core_ir/tests/simplify.rs` (`a_wildcard_arm_does_not_block_the_known_tags` のコメント)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`Binding::Shared` の doc コメント、132行目付近)
- Modify: `crates/eml_interp/src/machine.rs` (89行目付近の内部の誤りの文言)
- Modify: `crates/eml_interp/tests/data.rs` (`an_arm_with_a_different_number_of_fields_is_an_internal_error`)
- Modify: `crates/eml_core_ir/src/simplify.rs` (`split_known_tags` の `target`)
- Modify: `docs/implementation/test-changes.md`

**Interfaces:**
- Consumes: Task 1 の後のコード (作業用の設計文書を指さない)

**期待値の変わるテスト:** 種類1: `crates/eml_interp/tests/data.rs` の `an_arm_with_a_different_number_of_fields_is_an_internal_error` の期待する文言と、テストの名前。ほかは変えない

- [ ] **Step 1: 内部の誤りの文言のテストを直す (先に失敗させる)**

`crates/eml_interp/tests/data.rs` の `an_arm_with_a_different_number_of_fields_is_an_internal_error` を `a_case_with_a_different_number_of_fields_is_an_internal_error` にし、期待する文言を次にする。

```rust
            fault: Fault::Internal(
                "a switch case binds a different number of fields than the value has"
            ),
```

Run: `cargo test -p eml_interp --test data a_case_with_a_different_number_of_fields`
Expected: FAIL (今の文言は「a switch arm binds …」)。

- [ ] **Step 2: 文言を直す**

`crates/eml_interp/src/machine.rs` の `"a switch arm binds a different number of fields than the value has"` を `"a switch case binds a different number of fields than the value has"` にする。Core IR の `Switch` の枝は R7e-2 で `Case` になり、同じ関数のもう1つの文言 (`"a switch without a matching case"`) も「case」だからである。

Run: `cargo test -p eml_interp --test data a_case_with_a_different_number_of_fields`
Expected: PASS。

- [ ] **Step 3: 古いコメントを直す**

- `crates/eml_core_ir/tests/simplify.rs` の `a_wildcard_arm_does_not_block_the_known_tags` のコメント (「決定木が `if` の join point の本体の中に置いた残りの枝の join point を F が外へ出すので、… (docs/implementation/status.md にあった制限)」) を、今のスナップショットが確かめていることに合わせる。決定木は行列に現れないコンストラクタを `default` にまとめ、残りの枝の join point を作らない。このテストは、ワイルドカードの枝があっても B2 が `switch` に届き、`if` の結果で分岐し直さないことを確かめる。status.md の制限への言及は消す (その制限は R7e-2 でなくなった)
- `crates/eml_core_ir/src/translate/mod.rs` の `Binding::Shared` の doc コメントの「`match` の枝と、決定木の残りの部分木に使う」を、今の使い方に合わせる。`grep -rn "Binding::Shared" crates/eml_core_ir/src` で使う箇所を確かめる (`match` の枝のうち2つ以上の葉か0の葉から届くもの、`let` と引数のパターンの分解など)。決定木の残りの部分木には使わなくなった

- [ ] **Step 4: B2 で値の行き先を1回だけ求める**

`crates/eml_core_ir/src/simplify.rs` の `split_known_tags` で、`target(value)` を値ごとに3回求めている (切り出せるかの判定、`targeted`、jump の付け替え)。jump の位置と同じ並びの `Vec` に、各値の行き先を1回だけ求めて持ち、3か所はそれを読む形にする。ふるまいは変えない。型の入れ子が深くなるなら、行き先を表す小さな型 (たとえば `enum Destination { Unknown, Blocked, To(Option<u32>) }`) を足してよい。どちらが読みやすいかは実装の判断で決め、報告に書く。

Run: `cargo test -p eml_core_ir`
Expected: すべて PASS。スナップショットは変わらない。

- [ ] **Step 5: テストを流して記録し、コミットする**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS。`git diff --no-ext-diff --stat -- crates/*/tests crates/eml_cli/tests/snapshots tests` に出るのは `crates/eml_interp/tests/data.rs` と `crates/eml_core_ir/tests/simplify.rs` (コメントだけ) である。

`docs/implementation/test-changes.md` の末尾に節を足す。

```markdown
### リファクタリング R7f

- 種類1: Core IR の `Switch` の枝は R7e-2 で `Case` になったので、インタプリタの内部の誤りの文言を「a switch case binds a different number of fields than the value has」にそろえた。`eml_interp/tests/data.rs` の `an_arm_with_a_different_number_of_fields_is_an_internal_error` を `a_case_with_a_different_number_of_fields_is_an_internal_error` にし、期待する文言を変えた
```

```bash
git add crates docs/implementation/test-changes.md
git commit -m "Fix the minors left from R7e-2: stale comments, the case wording and B2's repeated lookup

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```

---

### Task 3: `architecture.md` を簡素にする

**Files:**
- Modify: `docs/implementation/architecture.md`
- 必要なら Modify: `docs/future/roadmap.md` (Task 4 で扱うので、ここでは変えない)

**Interfaces:**
- Consumes: Task 1 で足した「プログラム全体の構成」の節 (簡素化の対象にしない)

ロードマップの「文書の簡素化」(`docs/future/roadmap.md`) のとおり、コードを変えるたびに食い違う細部の説明を減らす。今の `architecture.md` は約61KB (268行) で、多くの箇条書きがコードの型、欄、関数の名前を並べて言い換えている。

- [ ] **Step 1: 残すものと消すものを決める**

書く前に `yomiyasu:yomiyasu` スキルを読み込む。各節の箇条書きを、次の基準で分ける。

- 残す
  - 段階の境目と、段階の間で渡すもの (各段階の入口の関数、出力の型の名前)
  - 決定と、その理由 (「〜のため」「〜しないのは〜だから」)。とくに、コードを読んでも分からない理由
  - 段階をまたぐ規律 (誤りでも止まらない、`Table::export` の規律、評価の順を1か所に置くこと、`captures` をつねに正しくすること など)
  - コードのコメントが指す見出し (Global Constraints) と、`Table::export` の規則
  - どのファイルに何があるかの短い地図 (1つのファイルにつき1行まで)
- 減らす
  - 型、欄、関数、診断の番号の列挙で、コードを読めば分かるもの。理由のない「`X` は `Y` を持つ」の文
  - アルゴリズムの手順の説明で、コードのコメントに同じことが書いてあるもの
  - `docs/spec/` に書いてあることの繰り返し (spec を指す1文に置き換える)
  - 段階ごとの「いつ足したか」の経緯 (`status.md` と git の履歴にある)

目安は、全体を半分ほど (約30KB) にし、1つの節の箇条書きを10個程度まで、1つの箇条書きを3文程度までにすることである。数字に合わせるために、決定とその理由を消さない。

- [ ] **Step 2: 書き直す**

節の並びと見出しは今のままにする (「プログラム全体の構成」を含む)。各節を Step 1 の基準で書き直す。書き直した後に次を確かめる。

Run: `grep -rn "architecture.md の「" crates docs --exclude-dir=superpowers`
Expected: 出た各行が指す見出しか規則が、書き直した `architecture.md` にある。

Run: `wc -c docs/implementation/architecture.md`
Expected: 書き直す前 (約61KB) よりはっきり小さい。目安は約30KB。

消した箇条書きのうち、決定とその理由を含むものは、残した箇条書きに1文で入れたか、コードのコメントか `docs/spec/` にすでに同じ理由があることを確かめる。消した理由の一覧 (箇条書きの要約と、消した理由か移した先) を報告に書く。

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/architecture.md`
Expected: 英単語の前後の空白と箇条書きの比率の指摘だけ。文末のコロンがあれば直す。

- [ ] **Step 3: コミットする**

```bash
git add docs/implementation/architecture.md
git commit -m "Trim the architecture notes to stage boundaries, decisions and their reasons

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```

---

### Task 4: R7 を完了にし、作業用の文書を削除する

**Files:**
- Modify: `docs/implementation/status.md`
- Modify: `docs/future/roadmap.md`
- Delete: `docs/superpowers/specs/2026-10-06-refactor-r7-design.md`
- Delete: `docs/superpowers/plans/2026-10-06-refactor-r7a.md`、`2026-10-06-refactor-r7b-1.md`、`2026-10-06-refactor-r7b-2.md`、`2026-10-06-refactor-r7b-3.md`、`2026-10-07-refactor-r7c.md`、`2026-10-07-refactor-r7d.md`、`2026-10-07-refactor-r7e-1.md`、`2026-10-07-refactor-r7e-2.md`

**Interfaces:**
- Consumes: Task 1〜3 の後の文書

- [ ] **Step 1: `status.md` を R7 の完了に合わせる**

書く前に `yomiyasu:yomiyasu` スキルを読み込む。

- 「リファクタリング」の表の R7 の行を「完了」にし、内容の欄を、R7a〜R7f で何をしたかの短い要約にする (今の行の内容を、回ごとに1文程度で)
- 「R7 で直す項目」の節を、R5 と R6 の節と同じ形の短い段落にする (「R7 で済んだ。…」)。S2 に回したもの (import をたどるローダ、複数ファイルの fixture、ディレクトリを1件とする UI テスト、import の循環とモジュールの根の決定) が S2 の項目にあることを確かめ、なければ S2 の項目に移す
- 「完了した作業」の表に「リファクタリング R7」の行を足す。ほかの行と同じく、何をしたかを2〜3文で書く
- 各 crate の行 (`eml_syntax`〜`eml_interp`) のうち、R7 の回の経緯を並べている部分は、今の状態の説明として読めるように短くする (経緯は「完了した作業」と git の履歴にある)

- [ ] **Step 2: ロードマップを直す**

`docs/future/roadmap.md` の「文書の簡素化」の項目は、Task 3 で済んだので消す。

- [ ] **Step 3: 作業用の文書を削除する**

```bash
git rm docs/superpowers/specs/2026-10-06-refactor-r7-design.md \
  docs/superpowers/plans/2026-10-06-refactor-r7a.md \
  docs/superpowers/plans/2026-10-06-refactor-r7b-1.md \
  docs/superpowers/plans/2026-10-06-refactor-r7b-2.md \
  docs/superpowers/plans/2026-10-06-refactor-r7b-3.md \
  docs/superpowers/plans/2026-10-07-refactor-r7c.md \
  docs/superpowers/plans/2026-10-07-refactor-r7d.md \
  docs/superpowers/plans/2026-10-07-refactor-r7e-1.md \
  docs/superpowers/plans/2026-10-07-refactor-r7e-2.md
```

R7f の計画 (`docs/superpowers/plans/2026-10-07-refactor-r7f.md`) は削除しない (最終の見直しの後にコントローラが削除する)。

Run: `grep -rn "2026-10-06-refactor-r7\|refactor-r7[a-f]" crates docs --exclude-dir=superpowers`
Expected: 何も出ない。

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/status.md`
Expected: 英単語の前後の空白と箇条書きの比率の指摘だけ。

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS。

- [ ] **Step 4: コミットする**

```bash
git add docs
git commit -m "Close refactor R7: record it as done and delete its working documents

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01RSkNuf5aBtsdHwHZ92gz6y"
```
