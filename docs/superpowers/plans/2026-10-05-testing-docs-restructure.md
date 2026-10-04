# テストの文書の整理 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `docs/implementation/testing.md` を、今のテストの書き方と置き場所だけを書く手引きに組み直す。変更の記録は `test-changes.md` に、マルチコアのテスト方針は `future/multicore.md` に移す。

**Architecture:** 文書だけを変える。最初に記録を文字を変えずに移し (Task 1)、次にマルチコアの節を移す (Task 2)。その後、testing.md に置き場所の規則と地図と UI テストの分類を書き (Task 3)、周りの文書のリンクと status.md を直す (Task 4)。最後に作業用の spec と計画を消す (Task 5)。

**Tech Stack:** Markdown。確認には `git`、`diff`、`python3`、`cargo test` を使う。

**Spec:** `docs/superpowers/specs/2026-10-05-testing-docs-restructure-design.md`

## Global Constraints

- 作業は `main` から切ったブランチ `testing-docs` で行う。タスクごとにコミットする。コミットメッセージは英語で、末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
  ```

- テストのコードとテストのファイル (`crates/**/tests/`、`crates/**/src/` の `#[cfg(test)]`、`tests/ui/`) を変えない
- 記録の項目 (testing.md の `### 構文の段階 S1` から `### join point の解析の整理` の最後の箇条まで) は、1文字も変えずに移す
- 日本語を書く前に `yomiyasu:yomiyasu` スキルを読み、その規則に従う (CLAUDE.md)。ただし、英単語の前後の半角空白 (`UI テスト`、`crate ごと`) は、リポジトリの文書の書き方に合わせて残す。文体は既存の docs と同じ常体 (である調) にする
- このプランのステップに書いた本文をそのまま使う。本文の誤り (リンク先の取り違え、事実との食い違い) を見つけたら、直してからコミットメッセージに理由を書く
- 確認の基準は `main` である。作業中は `main` を動かさないので、`git show main:...` で作業前の文書を読める。シェルの変数はステップをまたいで残らないので、基準を変数に入れない

## Review Focus

- 移した記録の本文が、空白や改行も含めて元と同じでない。記録は合意の証拠なので、1文字でも変わると履歴が信用できなくなる → Task 1 の Step 4 の `diff`
- 記録の中の相対リンク (`../spec/lexical.md` など) が、移した先で別の場所を指す。test-changes.md は testing.md と同じディレクトリなので同じ所を指すはずだが、確かめる → Task 4 の Step 3 のリンクの検査
- 「下の「テストの変更の記録」」や「「マルチコア対応の段階のテスト」」のような、消した節を名前で指す文が残る → Task 1 の Step 5 と Task 2 の Step 3 の `grep`
- テストの地図から、テストを持つファイルが漏れる。地図は新しいテストの置き場所を決めるのに使うので、漏れると読み手が既存のファイルを見落とす → Task 3 の Step 4 の網羅の検査
- docs/README.md の文書の地図と、各文書の冒頭の「位置づけ」が食い違う → Task 4 の Step 1 と Step 3

---

### Task 1: 変更の記録を test-changes.md に移す

**Files:**
- Create: `docs/implementation/test-changes.md`
- Modify: `docs/implementation/testing.md` (冒頭の段落、「テストの変更の運用」の表、「テストの変更の記録」の節)
- Modify: `CLAUDE.md` (Testing の節)

**Interfaces:**
- Consumes: なし
- Produces: `docs/implementation/test-changes.md`。見出し `## 記録` の下に、元の `###` の見出しのまま項目が並ぶ。Task 4 がこのファイルを docs/README.md の地図に載せる

- [ ] **Step 1: ブランチを切る**

```bash
cd /Users/arakaki/Projects/eml
git switch -c testing-docs main
```

- [ ] **Step 2: test-changes.md を作る**

次の冒頭を書き、その後ろに、testing.md の `### 構文の段階 S1` の行から、`## よく使うコマンド` の直前の空行の手前までを、そのまま貼る。貼るのはエディタでなくコマンドで行い、文字の変化を防ぐ。

冒頭 (`docs/implementation/test-changes.md`):

```markdown
# テストの変更の記録

位置づけ: 記録。

[テスト戦略](testing.md) の「テストの変更の運用」で決めた種類1と種類2の変更を、作業ごとに記録する。

## 書き方

- 作業ごとに見出しを1つ立てる
- 変更ごとに箇条を1つ書き、種類 (1か2)、変えた理由、変わらない目的を書く

## 記録

```

貼り付け:

```bash
awk '/^### 構文の段階 S1$/{on=1} /^## よく使うコマンド$/{on=0} on' docs/implementation/testing.md \
  | sed -e :a -e '/^\n*$/{$d;N;ba' -e '}' >> docs/implementation/test-changes.md
```

`sed` の部分は、末尾の空行を落とす。ファイルの最後は改行1つで終わる。

- [ ] **Step 3: testing.md から記録の節を除き、行き先を変える**

`docs/implementation/testing.md` で次のように変える。

1. `## テストの変更の記録` の行から、`## よく使うコマンド` の直前までを削除する。`## よく使うコマンド` の前には空行を1つ残す
2. 冒頭の段落 (5行目) を次に置き換える

   ```markdown
   テストの書き方、テストの変更の運用、層ごとの方法、UI テストの仕組みを定める。テストの変更の記録は [test-changes.md](test-changes.md) にある。マルチコア対応の段階でのテスト方針も扱う。
   ```

   最後の1文は Task 2 で消す。

3. 「テストの変更の運用」の表の、種類1と種類2の「合意と記録」の欄を次にする

   ```markdown
   | 1. 振る舞いの変更 | 言語として観測できる期待値。UI テストの出力、診断の番号と文言、成功か失敗か。テストの削除と移動もここに入れる | 事前に合意を取り、[test-changes.md](test-changes.md) に理由を書く |
   | 2. 内部表現の変更 | 中間表現のダンプなど、内部の設計を写したスナップショットの期待値 | 作業の spec に、変わるテストと理由を列挙する。spec の承認を合意とみなし、[test-changes.md](test-changes.md) に書く |
   ```

- [ ] **Step 4: 移した本文が元と同じことを確かめる**

```bash
diff <(git show main:docs/implementation/testing.md \
        | awk '/^### 構文の段階 S1$/{on=1} /^## よく使うコマンド$/{on=0} on' \
        | sed -e :a -e '/^\n*$/{$d;N;ba' -e '}') \
     <(awk '/^### 構文の段階 S1$/{on=1} on' docs/implementation/test-changes.md) \
  && echo IDENTICAL
```

Expected: `IDENTICAL` だけが出る。

- [ ] **Step 5: CLAUDE.md を直し、消した節への参照が残っていないことを確かめる**

`CLAUDE.md` の Testing の節の1文を変える。

```diff
-... (3) mechanical follow-ups that keep every expected value byte-identical are allowed when the plan says so. Kinds 1 and 2 are recorded in `testing.md`.
+... (3) mechanical follow-ups that keep every expected value byte-identical are allowed when the plan says so. Kinds 1 and 2 are recorded in `docs/implementation/test-changes.md`.
```

```bash
grep -rn '「テストの変更の記録」' --exclude-dir=target --exclude-dir=superpowers . ; echo "exit=$?"
```

Expected: 何も出ず `exit=1`。

- [ ] **Step 6: コミットする**

```bash
git add docs/implementation/test-changes.md docs/implementation/testing.md CLAUDE.md
git commit -F - <<'EOF'
Move the record of test changes out of the testing guide

The record grows with every piece of work and buried the guide. Entries move byte-identical.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 2: マルチコアのテスト方針を future/multicore.md に移す

**Files:**
- Modify: `docs/implementation/testing.md` (冒頭の段落、`## マルチコア対応の段階のテスト` の節)
- Modify: `docs/future/multicore.md:260-262` (`### テスト` の節)

**Interfaces:**
- Consumes: Task 1 の後の testing.md の冒頭の段落
- Produces: なし

- [ ] **Step 1: multicore.md の「テスト」の節を書き換える**

`docs/future/multicore.md` の `### テスト` の下の1文 (`テストの方針は [テスト戦略](../implementation/testing.md) の「マルチコア対応の段階のテスト」にまとめてある。`) を、次に置き換える。箇条の3行は testing.md の該当の節の箇条と同じ文字にする。

```markdown
マルチコア対応の段階では、次の方針でテストする。テスト全体の方針は [テスト戦略](../implementation/testing.md) にある。

- 本物のマルチコアで動かすと、並行処理の出力の順序が変わり、UI テストのスナップショットが安定しない。UI テストは `threads = 1` か、シードを固定した決定的なシミュレーションで実行する
- `debug_heap` を拡張し、共有の印のないオブジェクトを所有者でないスレッドから触ったら検出する。印の付け忘れを見つけるためで、リーク検出や解放済みアクセスの検出と同じ位置づけにする。所有者は、`debug_heap` が有効なときだけ別のテーブルに記録する
- ランタイムの `unsafe` な部分 (あれば) は、Miri と loom (並行処理のモデル検査) でテストする
```

- [ ] **Step 2: testing.md から節を除く**

`docs/implementation/testing.md` の `## マルチコア対応の段階のテスト` の行から末尾までを削除する。ファイルは `## よく使うコマンド` のコードブロックの閉じ (```` ``` ````) と改行1つで終わる。冒頭の段落から「マルチコア対応の段階でのテスト方針も扱う。」を除く。

- [ ] **Step 3: 箇条が同じことと、参照が残っていないことを確かめる**

```bash
diff <(git show main:docs/implementation/testing.md | awk '/^## マルチコア対応の段階のテスト$/{on=1} on' | grep '^- ') \
     <(awk '/^### テスト$/{on=1} /^## /{on=0} on' docs/future/multicore.md | grep '^- ') && echo IDENTICAL
grep -rn 'マルチコア対応の段階のテスト' --exclude-dir=target --exclude-dir=superpowers . ; echo "exit=$?"
```

Expected: `IDENTICAL`、続いて何も出ず `exit=1`。

- [ ] **Step 4: コミットする**

```bash
git add docs/implementation/testing.md docs/future/multicore.md
git commit -F - <<'EOF'
Move the multicore testing policy into the multicore design

Future designs live under docs/future/, so the policy belongs next to the design it tests.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 3: 置き場所の規則、テストの地図、UI テストの分類を書く

**Files:**
- Modify: `docs/implementation/testing.md` (冒頭の段落、`## 層ごとの方法` の節、`## UI テスト` の節。新しい `## テストの置き場所` の節)

**Interfaces:**
- Consumes: Task 1 と Task 2 の後の testing.md
- Produces: testing.md の節の名前「テストの置き場所」「UI テスト」。Task 4 の status.md の項目がこの名前で指す

- [ ] **Step 1: 冒頭の段落を直す**

```markdown
テストの書き方、テストの変更の運用、テストの置き場所、層ごとの方法、UI テストの仕組みを定める。テストの変更の記録は [test-changes.md](test-changes.md) にある。
```

- [ ] **Step 2: 「テストの置き場所」の節を足す**

`## テストの変更の運用` の節の後ろ、`## 層ごとの方法` の前に、次の節を入れる。

````markdown
## テストの置き場所

新しいテストの置き場所は、期待するものを上から順に見て、最初に当てはまる行で決める。

| # | 期待するもの | 置き場所 |
|---|---|---|
| 1 | プログラムを `eml check` / `eml run` にかけたときにユーザーが見るもの。stdout、実行時エラー、プログラム全体が通るか落ちるか | UI テスト (`tests/ui/`) |
| 2 | 1つの段階の出力 (CST、HIR、推論したシグネチャ、Core IR) と、その段階の診断の細部 (位置、回復、端のケース) | その段階の crate の結合テスト (`crates/<段階>/tests/`)。ソースから `eml_test_support` で組み立てる |
| 3 | フロントエンドからは作れない状態。壊れた IR、`decref` の抜けた IR など | `eml_core_ir/tests/verify.rs` か `eml_interp/tests/`。手書きの Core IR で組み立てる |
| 4 | 公開の API から届かないか、内部の状態を直接組まないと確かめにくい部品の振る舞い。レイアウト段、パーサのマーカー、単一化の表、Kind の制約の解消、ヒープなど | `src/` の単体テスト |
| 5 | lib API の流れ、CLI の終了コード、テスト補助そのもの | `eml_cli/tests/api.rs`、`eml_cli/tests/cli.rs`、`eml_test_support/tests/` |

### 重複させない

1つの事実は、それを観測できるいちばん下の層で確かめる。UI テストは機能ごとの代表的な筋書きを確かめ、段階の端のケースを繰り返さない。例えば診断なら、端のケースは段階の crate のテストに置き、代表的な表示を `check-fail/` に1つ置く。

### crate の中の置き方

- 結合テストは、話題ごとに1ファイルにする。ファイル名は spec の節か言語の機能から付ける (`effects.rs`、`operators.rs`)。1つのファイルが複数の話題にまたがったら、話題で分ける
- crate の結合テストが使う表示の関数は `tests/common/mod.rs` に置く
- 単体テストは、ファイルの末尾の `#[cfg(test)] mod tests` に置く。大きくなったら、`eml_types/src/table/tests.rs` のように隣の `tests.rs` に分ける
- `crates/eml_test_support/` は、結合テストのためにパイプラインを組む関数 (`parse`、`lower`、`check`、`core`、`run`、`execute`) と、診断を文字列にする関数 (`short`、`full`) を持つ。開発専用の crate で、各 crate の `tests/` からだけ使う。段階は feature (`hir` < `types` < `core` < `run`) で選び、各 crate は自分の段階までを有効にする。下流の crate がまだ組み立たなくても、上流の段階のテストを流せるようにするためである。`src/` の `#[cfg(test)]` から使うと、テストする crate が2つ別々にリンクされて型が合わなくなる

### 今あるテストの地図

| crate | 結合テスト (`tests/`) | 単体テスト (`src/`) |
|---|---|---|
| `eml_diagnostics` | なし | `lib.rs` (診断の番号と E0004)、`source.rs` (`SourceFiles` と行と列)、`render.rs` (診断の表示) |
| `eml_syntax` | `lexer.rs` (字句)、`literals.rs` (リテラルの値の解釈)、`parser.rs` (空のファイル、項目の間の回復、BOM と shebang)、`declarations.rs` (シグネチャ、型、row、`data`、`effect`、fixity)、`expressions.rs` (等式、パターン、式)、`control.rs` (`if` と `match`)、`handlers.rs` (handler)、`nesting.rs` (入れ子の深さの上限)、`ast.rs` (型付き AST ラッパ)、`corpus.rs` (コーパス。ソースは `tests/corpus/` にあり、`s1.em` は S1 の構文、`later_stages.em` は S2 以降の構文を含む) | `layout.rs` (レイアウト段)、`parser.rs` (パーサのマーカー、先読み、診断の位置)、`grammar/scan.rs` (回復の範囲の走査)、`syntax_kind.rs` と `token_set.rs` (構文の種類の表) |
| `eml_hir` | `lower.rs` (名前解決と脱糖)、`operators.rs` (演算子の組み直し)、`effects.rs` (エフェクトと操作)、`structure.rs` (HIR のデータ構造と走査) | `builtin.rs` (組み込みの表)、`lower/scope.rs` (名前空間) |
| `eml_types` | `check.rs` (推論と型の診断)、`effects.rs` (エフェクト、handler、継続) | `table/tests.rs` (単一化と型の書き出し)、`kind.rs` (Kind の制約の解消)、`ty.rs` (型の表示)、`scc.rs` (関数の呼び出しの強連結成分)、`check/mod.rs` (診断の文言) |
| `eml_core_ir` | `lower.rs` (Core IR への変換と `dup` / `decref` の位置)、`verify.rs` (手書きの Core IR による verifier) | なし |
| `eml_runtime` | なし | `heap.rs` (確保と解放、世代番号、リーク、フレームと継続の解放)、`output.rs` (`OutputSink`) |
| `eml_interp` | `run.rs` (手書きの Core IR や生成したソースによる実行)、`closures.rs` (手書きの Core IR によるクロージャ) | `lib.rs` (実行時エラーの表示と `RunConfig`) |
| `eml_cli` | `ui.rs` (UI テスト)、`api.rs` (lib API の流れ)、`cli.rs` (CLI の終了コード) | なし |
| `eml_test_support` | `support.rs` (テスト補助そのもの) | なし |
````

- [ ] **Step 3: 「層ごとの方法」を今ある層に絞る**

`## 層ごとの方法` の節を次に置き換える。表の「線形性」の行を今ある E3001 に絞り、「網羅性」の行を除く。表の後ろにあった「現在あるテストの置き場所は次のとおり。」から始まる一覧は、Step 2 の地図と「crate の中の置き方」に置き換わったので除く。

```markdown
## 層ごとの方法

| 層 | 方法 |
|---|---|
| 字句 | トークン列のスナップショット |
| パーサ | `insta` で CST をダンプしたスナップショット。壊れた入力から回復できるかのケースを多めに用意する |
| 名前解決と脱糖 | HIR の pretty printer で変換結果 (演算子の組み直しと脱糖を含む) をダンプしたスナップショット |
| 型推論 | 推論したシグネチャ (Kind と row を含む) のスナップショット |
| Core IR | Core IR の pretty printer で、`dup` / `decref` の位置を含めてダンプしたスナップショット |
| ランタイム | ヒープの単体テスト (確保と解放、世代番号による解放済みアクセスの検出、リークの数え方、長い連鎖の解放) |
| 診断 | 表示した診断テキストのスナップショット |
| 線形性 | 線形な値 (`once` の操作の `k` など) を1回でなく使う不正なプログラム (E3001) と、正しく通るべきプログラムの対 |
| 全体 (UI テスト) | 下の「UI テスト」 |
| CLI | `eml run` / `eml check` の終了コードと引数の誤りを数件確認する |
| RC | すべての実行テストで `debug_heap` を有効にする。違反があればテストを失敗させる |

### 後の段階で足すテスト

- 線形性 (段階5): 線形性の検査パスを分けたら、二重使用、消費漏れ、`_` での破棄、`multi` をまたぐ、継続の扱い忘れのそれぞれについて、不正なプログラムと正しく通るべきプログラムの対を足す
- 網羅性 (段階4): 網羅されていない `match`、到達しない枝、反駁可能な `let`
```

- [ ] **Step 4: 地図がテストを持つファイルを漏らしていないことを確かめる**

```bash
python3 - <<'EOF'
import pathlib, re, subprocess
doc = pathlib.Path("docs/implementation/testing.md").read_text()
section = doc.split("### 今あるテストの地図")[1].split("\n## ")[0]
rows = {}
for line in section.splitlines():
    m = re.match(r"\| `(eml_\w+)` \|(.*)\|(.*)\|", line)
    if m:
        rows[m.group(1)] = (m.group(2), m.group(3))
files = subprocess.run(["git", "ls-files", "crates"], capture_output=True, text=True).stdout.split()
missing = []
for f in files:
    p = pathlib.Path(f)
    crate = p.parts[1]
    if p.parts[2] == "tests" and p.suffix == ".rs" and p.parent.name == "tests":
        if f"`{p.name}`" not in rows.get(crate, ("", ""))[0]:
            missing.append(f)
    elif p.parts[2] == "src" and p.suffix == ".rs":
        text = pathlib.Path(f).read_text()
        if "#[test]" in text:
            rel = "/".join(p.parts[3:])
            if f"`{rel}`" not in rows.get(crate, ("", ""))[1]:
                missing.append(f)
print("MISSING:", missing) if missing else print("COMPLETE")
EOF
```

Expected: `COMPLETE`。`MISSING` が出たら、そのファイルを地図の行に足して、もう一度流す。

- [ ] **Step 5: 「UI テスト」の節に分類を足す**

`## UI テスト` の節の箇条の後ろ (`## CLI のテスト` の前) に、次の小節を足す。既存の箇条は変えない。

```markdown
### 分類

`run/`、`check-fail/`、`run-fail/` の下に、分類のサブディレクトリを切る。成功すべきか失敗すべきかは、今までどおり最上位のディレクトリで決まる。

- `run/` と `run-fail/` は言語の機能で分け、両方で同じ名前を使う
  - `basics/`: 値、演算子、`let`、`if`、短絡評価
  - `functions/`: クロージャ、高階関数、部分適用
  - `effects/`: エフェクト、`multi`、継続
  - `runtime/`: 実装の性質を確かめるテスト。メモリの解放、スタックの深さ、join point、末尾呼び出し
- `check-fail/` は、主なエラーの番号の範囲 ([診断](../spec/diagnostics.md) の「番号の範囲」) で分ける。機能で分けると、エフェクトの誤りのように E1xxx と E2xxx にまたがるものの置き場所が決まらないためである
  - `syntax/` (E0xxx)、`names/` (E1xxx)、`types/` (E2xxx)、`linearity/` (E3xxx)。網羅性 (E4xxx) の検査を実装したら `exhaustiveness/` を足す
  - E3001 は今は `eml_types` が出すが、出す crate ではなく番号の範囲に従って `linearity/` に置く
  - E0004 (まだ対応していない構文) は、どの段階が出しても `not-yet-supported/` に置く
- サブディレクトリの名前は、親の `check-fail` と同じくケバブケースにする
- テストのパスはスナップショットの名前になる。そのため、UI テストの移動はスナップショットの名前を変え、種類1の変更になる
```

- [ ] **Step 6: 節の並びと文体を確かめる**

```bash
grep -n '^## \|^### ' docs/implementation/testing.md
python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/testing.md
```

Expected: 見出しが次の順に並ぶ。

```
## 方針
## テストの変更の運用
## テストの置き場所
### 重複させない
### crate の中の置き方
### 今あるテストの地図
## 層ごとの方法
### 後の段階で足すテスト
## UI テスト
### 分類
## CLI のテスト
## よく使うコマンド
```

リンターの警告のうち、英単語の前後の半角空白と箇条書きの比率は Global Constraints のとおり残す。それ以外の警告 (文末コロン、太字の書き方など) は直す。

- [ ] **Step 7: コミットする**

```bash
git add docs/implementation/testing.md
git commit -F - <<'EOF'
Write down where tests go and how UI tests are grouped

The testing guide now says where a new test belongs, maps the existing tests, keeps only layers that exist in the per-layer table, and fixes the UI test categories that the later cleanup will apply.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 4: 文書の地図と status.md を直し、リンクを確かめる

**Files:**
- Modify: `docs/README.md:28` (文書の地図の testing.md の行と、その後ろに足す行)
- Modify: `docs/implementation/status.md` (「次の作業の注意点」の末尾の項目の後ろ)

**Interfaces:**
- Consumes: Task 1 の test-changes.md、Task 3 の testing.md の節の名前
- Produces: なし

- [ ] **Step 1: docs/README.md の地図を直す**

testing.md の行を次の2行に置き換える。

```markdown
| [implementation/testing.md](implementation/testing.md) | 手引き | テスト戦略、テストの置き場所、UI テスト、テストの変更の運用 |
| [implementation/test-changes.md](implementation/test-changes.md) | 記録 | 種類1と種類2のテストの変更の記録 |
```

- [ ] **Step 2: status.md に残りの作業を書く**

`docs/implementation/status.md` の `## 次の作業の注意点` の箇条の末尾 (`- 診断の言い方: HIR が呼び出しを1つにまとめるので、` で始まる項目の後ろ) に、次の項目を足す。

```markdown
- テストの整理の残り: テスト補助コードの統一 (各 crate の `tests/common` と `eml_test_support` の API) と、テスト本体の整理 (重複したテストと古くなったテストの削除、大きいファイルの分割、UI テストの分類) が残っている。既存のテストは、[testing.md](testing.md) の「テストの置き場所」の「重複させない」と、「UI テスト」の「分類」に、まだ合っていない
```

- [ ] **Step 3: リンクと節の名前を確かめる**

```bash
python3 - <<'EOF'
import pathlib, re
docs = ["CLAUDE.md", "docs/README.md", "docs/implementation/testing.md",
        "docs/implementation/test-changes.md", "docs/implementation/status.md",
        "docs/future/multicore.md"]
bad = []
for d in docs:
    p = pathlib.Path(d)
    for target in re.findall(r"\]\(([^)#\s]+)(?:#[^)]*)?\)", p.read_text()):
        if target.startswith("http"):
            continue
        if not (p.parent / target).exists():
            bad.append((d, target))
print("BROKEN:", bad) if bad else print("LINKS OK")
EOF
grep -c '^## テストの置き場所$\|^### 重複させない$\|^## UI テスト$\|^### 分類$' docs/implementation/testing.md
grep -n '^位置づけ' docs/implementation/testing.md docs/implementation/test-changes.md
```

Expected: `LINKS OK`、`4`、位置づけが testing.md は「手引き」、test-changes.md は「記録」で、docs/README.md の地図と一致する。

- [ ] **Step 4: テストが変わっていないことを確かめる**

```bash
git diff --stat main -- crates tests
cargo test 2>&1 | grep -E '^test result|FAILED|panicked' | sort | uniq -c
```

Expected: 1つ目のコマンドは何も出さない。2つ目は `test result: ok.` だけで、`FAILED` も `panicked` も出ない。

- [ ] **Step 5: コミットする**

```bash
git add docs/README.md docs/implementation/status.md
git commit -F - <<'EOF'
List the record of test changes and track the remaining test cleanup

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```

---

### Task 5: 作業用の文書を消す

**Files:**
- Delete: `docs/superpowers/specs/2026-10-05-testing-docs-restructure-design.md`
- Delete: `docs/superpowers/plans/2026-10-05-testing-docs-restructure.md`

**Interfaces:**
- Consumes: Task 1〜4 がすべて終わっていること
- Produces: なし

spec の「位置づけ」のとおり、決めた内容が testing.md と test-changes.md に入ったら作業用の文書を消す。残りのサブプロジェクトは status.md の項目 (Task 4) が指す。

- [ ] **Step 1: spec の内容が文書に入ったことを確かめる**

spec の各節について、入った先を確かめる。1章は Task 1、2、4、2章は Task 3 の Step 2 と Step 3、3章は Task 3 の Step 5、4章は Task 1 の Step 4、Task 3 の Step 4 と Step 6、Task 4 の Step 3 と Step 4 で確かめた。spec の「範囲の外」と「ユーザーと合意済みの決定」は、testing.md の「分類」(番号の範囲で分ける理由) と status.md の項目 (残りのサブプロジェクト) に入っている。

- [ ] **Step 2: 消してコミットする**

```bash
git rm docs/superpowers/specs/2026-10-05-testing-docs-restructure-design.md \
       docs/superpowers/plans/2026-10-05-testing-docs-restructure.md
git commit -F - <<'EOF'
Remove the working documents for the testing docs restructure

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01JwStHvur1XBMVUb8fznxgZ
EOF
```
