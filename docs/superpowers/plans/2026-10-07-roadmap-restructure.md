# ロードマップの組み直しの実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `docs/future/roadmap.md` を M2〜M9 のマイルストーンの列と、その後の項目に組み直し、ほかの docs とコメントの S1/S2/S3 の参照を M の番号に置き換える。

**Architecture:** 文書とコメントだけの変更である。roadmap.md を先に書き直し、そこに新しい節の名前 (「マイルストーンの列」「M2 モジュール」など) を作る。その後で、status.md と各文書の参照をその節に向ける。最後に、S1/S2/S3 の残りと、節への参照の切れを機械的に確かめる。

**Tech Stack:** Markdown (日本語)、Rust のコメント、`grep`、`python3` (参照の検査の一時スクリプト)、`cargo test`

**Spec:** `docs/superpowers/specs/2026-10-07-roadmap-restructure-design.md`

## Global Constraints

- 成果物は文書とコメントの変更だけである。コードの振る舞いとテストの期待値は変えない
- docs とコードのコメントは日本語で、である調で書く。日本語を書く前に `yomiyasu:yomiyasu` のスキルを読み、その規則に従う。英単語やインラインコードの前後に半角空白を入れる今の docs の書き方は保つ
- `CLAUDE.md` は英語で書く
- M2〜M9 は「マイルストーン」と呼ぶ。パイプラインの段階 (crate) を指す「段階」と混ぜない
- `crates/eml_syntax/tests/corpus.rs` の定数 `S1`、テスト名 `s1_corpus_*`、`crates/eml_syntax/tests/corpus/s1.em` (ファイル名と先頭のコメント) は変えない
- 1つの事実は1つの文書にだけ書く。ほかの文書からはリンクで示す
- コミットメッセージの末尾に次の2行を付ける

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01NQY7m9oreBjUG1cr17avCh
  ```

## Review Focus

- 節への参照の切れ: ほかの文書が「構文の段階」「S2 の材料」「処理系」などの節を名前で引いている。節を消したり名前を変えたりしたら、引いている側も直っていること (Task 7 の参照の検査で確かめる)
- spec の決定の書き違い: roadmap の M5 の「決めたこと」と「論点」が spec と食い違わないこと。特に `!=`、`negate`、既定のメソッド、表示用のクラスは論点であり、決めたことに書かない (Task 1 の Step 5 で突き合わせる)
- S1/S2/S3 の取り残し: docs、`CLAUDE.md`、crates に古い名前が残らないこと (Task 7 の grep で確かめる)
- 事実の重複: status.md から移した「S2 の材料」が、roadmap と status.md の両方に残らないこと (Task 2 の Step 4 で確かめる)
- 日本語の質: 文末のコロン、ダッシュ記号、絵文字がなく、yomiyasu のリンターで新しい警告の種類が増えていないこと (各タスクのリンターの手順で確かめる)

---

## 参照の検査スクリプト

Task 1 の前に、一時スクリプトを scratchpad に置く。リンク `[...](path)` の直後に `の「X」` と節の名前を引いている箇所を集め、リンク先のファイルに見出し `X` があるかを調べる。リポジトリには入れない。

`/private/tmp/claude-501/-Users-arakaki-Projects-eml/54b9c476-d7b7-4ebd-ba96-b6efde49ca57/scratchpad/check_refs.py`:

```python
import pathlib
import re
import sys

ROOT = pathlib.Path(sys.argv[1])
LINK = re.compile(r"\[[^\]]*\]\(([^)#]+\.md)\)\s*の「([^」]+)」")
HEADING = re.compile(r"^#+\s+(.*?)\s*$")


def headings(path):
    return {m.group(1) for line in path.read_text().splitlines() if (m := HEADING.match(line))}


def main():
    bad = []
    for md in ROOT.glob("docs/**/*.md"):
        if "superpowers" in md.parts:
            continue
        for lineno, line in enumerate(md.read_text().splitlines(), 1):
            for target, name in LINK.findall(line):
                path = (md.parent / target).resolve()
                if not path.exists():
                    bad.append(f"{md}:{lineno}: missing file {target}")
                elif name not in headings(path):
                    bad.append(f"{md}:{lineno}: no heading 「{name}」 in {target}")
    print("\n".join(bad) if bad else "OK")
    sys.exit(1 if bad else 0)


main()
```

`の「X」` が見出しではなく項目の名前を指す箇所 (「実行時エラーの位置」の項目、「`use`」など) も拾う。Task 0 で今の検査結果を保存し、Task 7 で、増えた行がないことを確かめる。

### Task 0: 検査の基準を取る

**Files:**
- Create: `/private/tmp/claude-501/-Users-arakaki-Projects-eml/54b9c476-d7b7-4ebd-ba96-b6efde49ca57/scratchpad/check_refs.py` (上のスクリプト)

- [ ] **Step 1: スクリプトを置く**

上のスクリプトを Write で置く。

- [ ] **Step 2: 今の結果を保存する**

Run:
```bash
cd /Users/arakaki/Projects/eml && python3 /private/tmp/claude-501/-Users-arakaki-Projects-eml/54b9c476-d7b7-4ebd-ba96-b6efde49ca57/scratchpad/check_refs.py . > /private/tmp/claude-501/-Users-arakaki-Projects-eml/54b9c476-d7b7-4ebd-ba96-b6efde49ca57/scratchpad/refs_before.txt; cat /private/tmp/claude-501/-Users-arakaki-Projects-eml/54b9c476-d7b7-4ebd-ba96-b6efde49ca57/scratchpad/refs_before.txt
```
Expected: 見出しでない項目を引く行だけが並ぶ (例: 「実行時エラーの位置」)。この一覧が基準になる。

- [ ] **Step 3: 今のテストが通ることを確かめる**

Run: `cd /Users/arakaki/Projects/eml && cargo test 2>&1 | grep -E "^test result" | awk '{s+=$4; f+=$6} END {print "passed", s, "failed", f}'`
Expected: `failed 0`。passed の数を控える (Task 6 で同じ数になること)。

---

### Task 1: roadmap.md を書き直す

**Files:**
- Modify: `docs/future/roadmap.md` (全体)

**Interfaces:**
- Produces: 見出し `## マイルストーンの進め方`、`## マイルストーンの列`、`## M2 モジュール`、`## M3 レコード、リスト、文字列`、`## M4 数値と文字`、`## M5 型クラス`、`## M6 REPL`、`## M7 バイト列と入出力`、`## M8 UTF-8 と標準ライブラリ`、`## M9 コマンドリテラル`、`## マイルストーンの後の項目` と、その下の `### 型システム`、`### 言語機能と構文`、`### 処理系`、`### マルチコア対応`。後のタスクはこの見出しの名前を `の「…」` で引く

- [ ] **Step 1: yomiyasu のスキルを読む**

`yomiyasu:yomiyasu` を Skill で呼ぶ。

- [ ] **Step 2: 冒頭と進め方を書く**

ファイルの先頭を次にする。

```markdown
# ロードマップ

位置づけ: 将来の設計。

今後の実装を、M1 に続くマイルストーン M2〜M9 の列と、その後の項目に分けてまとめる。今の言語の範囲は [実装の現在地](../implementation/status.md) の「今の言語の範囲」にある。

## マイルストーンの進め方

- 各マイルストーンは、spec、計画、実装の順に進める。spec は、この文書のそのマイルストーンの節を出発点にする
- 「決めたこと」は方針で、細部はそのマイルストーンの spec で決める。「論点」は、そのマイルストーンの spec で決める
- マイルストーンを終えたら、決まったことを `spec/` に移し、[実装の現在地](../implementation/status.md) を更新し、この文書からそのマイルストーンの節を削る
- マイルストーンは、パイプラインの段階 (crate) とは別の呼び方である
```

- [ ] **Step 3: マイルストーンの列を書く**

spec の「マイルストーンの列」の節から、表と「順序の理由」の箇条書きをそのまま写す。見出しは `## マイルストーンの列` にし、表の後に「順序の理由は次のとおり。」と箇条書きを置く。

- [ ] **Step 4: M2〜M9 の節を書く**

各節は次の形にする。

```markdown
## M2 モジュール

前提: なし。

(中身の段落)

### 決めたこと

- …

### 論点

- …
```

- 前提と中身は、spec の「マイルストーンの列」の表の行から写す。中身は表のセルの文を段落にする
- 「決めたこと」と「論点」は、spec の「各マイルストーンの方針と論点」の該当する節から写す。リンクの相対パスは roadmap.md の位置に合わせる (`../../spec/` → `../spec/`、`../../future/stdlib.md` → `stdlib.md`)
- M2 と M3 は spec では1つの節なので、次のように分ける
  - M2 の決めたこと: 「今の `Int`、`String`、`Bool` の `==` を参照ごとの具体化の表に移すだけで、振る舞いを変えない」
  - M3 の決めたこと: 「補間の穴は `String` に限り、M5 で表示用のクラスに広げる」
  - M2 と M3 と M9 の論点には、`docs/implementation/status.md` の「S2 の材料」の箇条書きを、文をそのままにして次のとおり移す。status.md の中の「上の「既知の制限」」は「[実装の現在地](../implementation/status.md) の「既知の制限」」に直す。リンクの相対パスは同じ深さなので変えない
    - M2 へ: 「参照ごとの具体化を記録する表を作り、`==` の比べ方を一般化する」「同じ名前の別の型を区別して表示する方法 (…)」「import の循環を許すかどうかと、モジュールの根」「import をたどるローダ、複数ファイルの fixture、ディレクトリを1件とする UI テスト」、および「spec で決めてあり実装を待つもの」のうち「import の並びでコンストラクタを `T(..)` でだけ取り込む規則 ([モジュールと名前解決](../spec/modules.md))」
    - M3 へ: 「row の仕組みを sort 付きの1つにし、…」「名前付きのレコードの実行時の表し方 (…)」「文字列のトークンを lexer のモードで分ける。…」「レイアウト規則3とレコードの `with` の衝突を解く」「借用のオペランドと `Field` を決める」「`::` の fixity」「補間、レコード、リストの CST」、および「spec で決めてあり実装を待つもの」のうち「タプルの射影 `t.0`、名前付きのレコード、射影と更新の線形性の規則 ([直積型とレコード](../spec/records.md))」
    - M9 へ: 「コマンドリテラルの CST」
- M5 の節の末尾に `### 方式を選んだ理由` を置き、spec の「アドホック多相の方式を選んだ理由」の表と段落を写す。理由を残すのは、spec を消した後も、M5 の spec を書く人が採らなかった案とその理由を読めるようにするためである

- [ ] **Step 5: M5 の決めたことと論点を spec と突き合わせる**

Run:
```bash
cd /Users/arakaki/Projects/eml && grep -nE '`!=`|negate|既定のメソッド|Show` と `Display|表示用のクラス' docs/future/roadmap.md
```
Expected: `!=` の扱い、`negate` を見せるか、既定のメソッドの時期、`Show` と `Display` を分けるかが、M5 の「論点」の行にだけ出る。「決めたこと」の行に出るのは「`Num` (前置の `-` を含む)」と「表示用のクラス」の語だけである。食い違ったら spec に合わせて直す。

- [ ] **Step 6: マイルストーンの後の項目を書く**

見出しを `## マイルストーンの後の項目` にし、次の段落で始める。

```markdown
M2〜M9 の後に扱う項目を、分野ごとにまとめる。方針まで決まっている項目と、論点を挙げただけの項目がある。詳しい設計がある項目は、その文書へのリンクを付けた。「前提」は、先に済ませておくマイルストーンか項目である。前提を書いていない項目は、どのマイルストーンの後でも着手できる。
```

その下に、今の roadmap.md の `## 型システム`、`## 言語機能と構文`、`## 処理系`、`## マルチコア対応` を `###` に下げて写し、次のとおり直す。前提は、項目の最後の文の後に「前提: …。」として足す。

`### 型システム`
- 「アドホック多相: 型クラスやトレイト。…」の項目とその子の箇条書き (補間の穴の型、接頭辞付きリテラルの一般化、`==` と `!=`) を削る。接頭辞付きリテラルは下の言語機能と構文に移す
- 「浮動小数と文字: …」の項目を削る
- 「使い切り必須の型」に「前提: ユーザーによる Kind の記述。」を足す
- 「フィールドの関数型の線形性」に「前提: ユーザーによる Kind の記述。」を足す

`### 言語機能と構文`
- 「局所的な可変変数の糖衣構文」に「前提: 可変参照。」を足す
- 「線形値の受け渡しの糖衣」に「前提: M8。」を足す
- 「フィールドの変換の糖衣」に「前提: M3。」を足す
- 「抽象型」「再エクスポート」「優先順位グループ」に「前提: M2。」を足す
- 「モジュールシステムと標準ライブラリの範囲: …」の項目を、次の1項目に置き換える

  ```markdown
  - 標準ライブラリの API の範囲: M8 で最初のモジュールを作った後、`Fs`、`Path`、`Proc`、`Json` などのモジュールの API を別の spec で決める ([標準ライブラリへの申し送り](stdlib.md))。前提: M8。
  ```

- 次の2項目を、「仮置きの式」の前に足す

  ```markdown
  - 接頭辞付きリテラルの一般化: `re"..."` のように、ユーザーが接頭辞付きのリテラルを定義できるようにする。穴の型付けにアドホック多相が要る。前提: M3、M5。
  - or パターン、ガード、as パターン: パターンを広げる。網羅性の検査 ([網羅性](../spec/exhaustiveness.md)) と決定木の変換を一緒に広げる。
  ```

`### 処理系`
- 「LSP」に「前提: インクリメンタル化。」を足す
- 「ネイティブ化」に「前提: M4 (多相な位置での `Int` と `Float` の表現)。」を足す
- 「レコードのネイティブな表現」に「前提: M3、ネイティブ化。」を足す
- 「doc comment」に「前提: LSP。」を足す
- 「記述子」に「前提: ネイティブ化。」を足す
- 「HIR の位置を source map に移す」の後に次の項目を足す

  ```markdown
  - フォーマッタ: CST がロスレスなので、その上に作る。前提: M9 (本番の構文がそろってから)。
  ```

`### マルチコア対応`
- 要約の段落の「`Lin` な配列」を「`Lin` の `MutArray` と `freeze` (前提: M7、Perceus の reuse analysis)」に直す

- [ ] **Step 7: 移した項目が残っていないことを確かめる**

Run:
```bash
cd /Users/arakaki/Projects/eml && grep -nE 'アドホック多相: 型クラス|浮動小数と文字:|モジュールシステムと標準ライブラリの範囲|構文の段階|S2|S3|`Lin` な配列' docs/future/roadmap.md
```
Expected: 出力なし。

- [ ] **Step 8: リンターを走らせる**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/future/roadmap.md | grep -vE '英単語の前後|箇条書きの比率' | grep -E '^L[0-9]+ \[' `
Expected: 出力なし。英単語の前後の空白と箇条書きの比率は今の docs の書き方なので除く。出た警告は、意味を変えない範囲で直す (直す試みは2回まで)。

- [ ] **Step 9: コミットする**

```bash
cd /Users/arakaki/Projects/eml && git add docs/future/roadmap.md && git commit -q -m "Restructure the roadmap into milestones M2 to M9 and a backlog

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NQY7m9oreBjUG1cr17avCh"
```

---

### Task 2: status.md を今の状態だけにする

**Files:**
- Modify: `docs/implementation/status.md:5`、`:22-31` (まだないもの)、`:33-43` (構文の段階)、`:53`、`:66-86` (S2 の材料)

**Interfaces:**
- Consumes: Task 1 の見出し「マイルストーンの列」「M2 モジュール」「マイルストーンの後の項目」
- Produces: status.md の見出しは「今の言語の範囲」「既知の制限」(と、その下の今の小見出し) だけになる

- [ ] **Step 1: 冒頭を直す**

5行目を次にする。

```markdown
M1 (言語の全体を一通り通す vertical slice と、本番の構文の最初の部分) は完了した。次は M2 である ([ロードマップ](../future/roadmap.md) の「マイルストーンの列」)。この文書は、今の言語の範囲と既知の制限をまとめる。実装が進んだら更新する。
```

- [ ] **Step 2: 「まだないもの」を直す**

`### まだないもの` の箇条書きと、その後の「将来の拡張は…」の文を次にする。

```markdown
- 浮動小数と文字 (字句として予約し、使うと E0004。M4)
- まだ実装していない構文: モジュールと import (M2)、名前付きのレコード、リスト、補間、複数行の文字列、raw 文字列 (M3)、コマンドリテラル (M9)。字句と文法の置き場所を用意してあり、使うと E0004 (未対応) を出して回復する。どの層が E0004 を出すかは [文法](../spec/grammar.md) の「実装の段階」にある
- 型クラスなどのアドホック多相 (M5)
- REPL (M6)
- バイト列、配列、標準入出力のハンドル (M7)、標準ライブラリ (M8)
- 可変参照 `Ref`、表面の構文での Kind の記述 (関数型の線形性 `m` を含む)、or パターン、ガード、as パターン、Perceus の reuse analysis と借用の最適化、LSP、フォーマッタ、LLVM バックエンド ([ロードマップ](../future/roadmap.md) の「マイルストーンの後の項目」)

今後のマイルストーンと将来の拡張は [ロードマップ](../future/roadmap.md) にまとめてある。
```

- [ ] **Step 3: 「構文の段階」の節と「S2 の材料」の節を削り、参照を直す**

- `## 構文の段階` から、次の `## 既知の制限` の手前までを削る (E0004 の段落は Step 2 で「まだないもの」に移した)
- `## S2 の材料` から末尾までを削る (中身は Task 1 で roadmap の M2、M3、M9 に移した)
- 53行目の「直し方は下の「S2 の材料」にある」を「直し方は [ロードマップ](../future/roadmap.md) の「M2 モジュール」にある」にする

- [ ] **Step 4: 重複と取り残しがないことを確かめる**

Run:
```bash
cd /Users/arakaki/Projects/eml && grep -nE 'S[123]\b|構文の段階|S2 の材料|決めること|作るもの' docs/implementation/status.md; grep -c 'row の仕組みを sort 付きの1つにし' docs/future/roadmap.md docs/implementation/status.md
```
Expected: 1つ目の grep は出力なし。2つ目は `docs/future/roadmap.md:1` と `docs/implementation/status.md:0`。

- [ ] **Step 5: リンターを走らせる**

Run: `python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/status.md | grep -vE '英単語の前後|箇条書きの比率' | grep -E '^L[0-9]+ \['`
Expected: 出力なし (出たら Task 1 の Step 8 と同じく直す)。

- [ ] **Step 6: コミットする**

```bash
cd /Users/arakaki/Projects/eml && git add docs/implementation/status.md && git commit -q -m "Keep only the current state in status.md and point at the milestones

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NQY7m9oreBjUG1cr17avCh"
```

---

### Task 3: 入口の文書の用語を直す (overview.md、README.md、CLAUDE.md)

**Files:**
- Modify: `docs/overview.md:88-89`、`docs/README.md:29`、`:31`、`:55`、`CLAUDE.md:55`

**Interfaces:**
- Consumes: Task 1 の見出し「マイルストーンの列」

- [ ] **Step 1: overview.md の用語表を直す**

88行目と89行目を次の2行にする。

```markdown
| マイルストーン1 (M1) | 言語の全体を一通り通した最初の vertical slice と、本番の構文の最初の部分。完了している。今の範囲は [実装の現在地](implementation/status.md) にある |
| M2〜M9 | M1 に続くマイルストーン。M2 モジュール、M3 レコード・リスト・文字列、M4 数値と文字、M5 型クラス、M6 REPL、M7 バイト列と入出力、M8 UTF-8 と標準ライブラリ、M9 コマンドリテラルである ([ロードマップ](future/roadmap.md) の「マイルストーンの列」)。パイプラインの「段階」とは別の呼び方である |
```

- [ ] **Step 2: README.md を直す**

- 29行目の status の行の説明を「今の言語の範囲、既知の制限」にする
- 31行目の roadmap の行の説明を「今後のマイルストーン (M2〜M9) と、その後の項目 (型システム、言語機能と構文、処理系、マルチコア)」にする
- 55行目の「将来の論点の一覧は [future/roadmap.md](future/roadmap.md) を正とする。」を「今後のマイルストーンと将来の論点の一覧は [future/roadmap.md](future/roadmap.md) を正とする。」にする

- [ ] **Step 3: CLAUDE.md を直す**

55行目を次にする (英語)。

```markdown
- `eml_syntax` implements the grammar of milestone M1 (`docs/implementation/status.md`). Constructs of later milestones (modules in M2; records, lists and string interpolation in M3; command literals in M9) and the reserved float and char literals (M4) are lexed and parsed far enough to report E0004; where each one is reported is listed in `docs/spec/grammar.md`. The milestones are listed in `docs/future/roadmap.md`.
```

- [ ] **Step 4: 取り残しを確かめる**

Run: `cd /Users/arakaki/Projects/eml && grep -nE '\bS[123]\b|構文の段階|S2 の材料' docs/overview.md docs/README.md CLAUDE.md`
Expected: 出力なし。

- [ ] **Step 5: コミットする**

```bash
cd /Users/arakaki/Projects/eml && git add docs/overview.md docs/README.md CLAUDE.md && git commit -q -m "Name the milestones M2 to M9 in the overview, the docs map and CLAUDE.md

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NQY7m9oreBjUG1cr17avCh"
```

---

### Task 4: stdlib.md と multicore.md を決定にそろえる

**Files:**
- Modify: `docs/future/stdlib.md:11`、`:14`、`:16`、`:33`、`docs/future/multicore.md` (「`Lin` な配列」の3か所)

**Interfaces:**
- Consumes: Task 1 の見出し「M7 バイト列と入出力」「M8 UTF-8 と標準ライブラリ」「M5 型クラス」

- [ ] **Step 1: stdlib.md の構文の設計からの申し送りを直す**

- 11行目「同じ型のハンドルの取り違えを防ぐ: `Stdin` / `Stdout` / `Stderr` は別の型にする」を次にする

  ```markdown
  - 標準入出力のハンドル: `Stdin` / `Stdout` / `Stderr` は別の型にして、取り違えを防ぐ。ハンドルは `Lin` で、バッファ付きの読み書きもハンドルにする。intrinsic はハンドルに対する read/write の操作にとどめ、「行を流す」などのストリーム処理は M8 の標準ライブラリにエフェクトとして書く ([ロードマップ](roadmap.md) の「M7 バイト列と入出力」)
  ```

- 14行目「`Prelude` の範囲」の項目の末尾に「M5 の後は、クラス `Eq`、`Ord`、`Num` と表示用のクラスも入る。`show_int` は M5 までの中継ぎである ([ロードマップ](roadmap.md) の「M5 型クラス」)」を足す
- 16行目「補間の穴は当面 `String` のみ: そのため、各型に `show_*` 関数を揃える」を次にする

  ```markdown
  - 補間の穴は M5 まで `String` のみ: それまでは各型に `show_*` 関数を揃える。M5 で穴を表示用のクラスに広げる
  ```

- 16行目の後に次の項目を足す

  ```markdown
  - `Bytes` と UTF-8: `String` は常に妥当な UTF-8 である。intrinsic は、検証付きのデコード、エンコード、バイト長、バイト位置から `Char` と次の位置を読む操作、境界を検査する切り出しにとどめる。分割、検索、反復は eml で書く ([ロードマップ](roadmap.md) の「M8 UTF-8 と標準ライブラリ」)
  ```

- [ ] **Step 2: stdlib.md のマルチコア対応の設計からの申し送りを直す**

33行目「`Lin` な配列: …」を次にする。

```markdown
- 配列: `Array a` と `Bytes` は意味が不変の値で、RC が 1 ならその場で書き換える (M7)。分割と結合と並列の書き換えのために、`Lin` の `MutArray` を後で足す。`write : MutArray a -> Int -> a -> MutArray a` のように値を線形に受け渡す API と、分割と結合のプリミティブを持つ。Perceus の reuse analysis が前提になる
```

- [ ] **Step 3: multicore.md の3か所を直す**

Run: `cd /Users/arakaki/Projects/eml && grep -n 'Lin` な配列\|Array a' docs/future/multicore.md`
で3か所の行を確かめ、次のとおり直す。

- 「`Lin` の値を書き換えたい」の表の行: 「(例: `Lin` な配列の `write : Array a -> Int -> a -> Array a`)」を「(例: `Lin` の `MutArray` の `write : MutArray a -> Int -> a -> MutArray a`)」にする
- 「`Lin` な配列を重ならない部分に分けて」で始まる段落: 「`Lin` な配列」を「`Lin` の `MutArray`」にする
- 未決の論点の「**`Lin` な配列**: 分割と結合のプリミティブ、`freeze`。Perceus の reuse analysis が前提」を次にする

  ```markdown
  - **`Lin` の `MutArray`**: 分割と結合のプリミティブ、`freeze`。Perceus の reuse analysis が前提。普通の `Array` は `Unr` で、一意ならその場で書き換える ([ロードマップ](roadmap.md) の「M7 バイト列と入出力」)
  ```

- [ ] **Step 4: 取り残しを確かめる**

Run: `cd /Users/arakaki/Projects/eml && grep -n 'Lin` な配列\|当面 `String`' docs/future/stdlib.md docs/future/multicore.md`
Expected: 出力なし。

- [ ] **Step 5: リンターを走らせる**

Run: `for f in docs/future/stdlib.md docs/future/multicore.md; do python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.0.4/skills/yomiyasu/scripts/yomiyasu_lint.py $f | grep -vE '英単語の前後|箇条書きの比率' | grep -E '^L[0-9]+ \['; done`
Expected: 変更した行に新しい警告がない。

- [ ] **Step 6: コミットする**

```bash
cd /Users/arakaki/Projects/eml && git add docs/future/stdlib.md docs/future/multicore.md && git commit -q -m "Align the stdlib and multicore notes with Unr arrays, Lin handles and UTF-8

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NQY7m9oreBjUG1cr17avCh"
```

---

### Task 5: spec と implementation の文書の S1/S2/S3 を置き換える

**Files:**
- Modify: `docs/spec/examples.md:5,9,69,107,138,156`、`docs/spec/declarations.md:64,123,133,139,141,149`、`docs/spec/lexical.md:73,77,93`、`docs/spec/modules.md:5`、`docs/spec/core-ir.md:106`、`docs/spec/grammar.md:116,122,124,126,127`、`docs/spec/diagnostics.md:51,112`、`docs/spec/expressions.md:187`、`docs/spec/types.md:71`、`docs/spec/records.md:75,80,82`、`docs/implementation/architecture.md:28,120,191`、`docs/implementation/testing.md:57`

**Interfaces:**
- Consumes: Task 1 の見出し「マイルストーンの列」「M2 モジュール」「M4 数値と文字」「M5 型クラス」

- [ ] **Step 1: 置き換える**

各行で、左の文字列を右にする。左の文字列は行の中で一意である。

| 場所 | 今 | 新しく |
|---|---|---|
| examples.md:5 | `どの構文がどの段階 (S1、S2、S3) で実装されるかは [実装の現在地](../implementation/status.md) にある。` | `どの構文がどのマイルストーンで実装されるかは [ロードマップ](../future/roadmap.md) の「マイルストーンの列」にある。` |
| examples.md:9 | `import、名前付きのレコード、リストのパターン、補間、複数行の文字列 (S2) とコマンドリテラル (S3) を含む。` | `import (M2)、名前付きのレコード、リストのパターン、補間、複数行の文字列 (M3) とコマンドリテラル (M9) を含む。` |
| examples.md:69 | `補間 (S2) を含む。` | `補間 (M3) を含む。` |
| examples.md:107 | `リストのリテラルと補間 (S2)、コマンドリテラル (S3) を含む。` | `リストのリテラルと補間 (M3)、コマンドリテラル (M9) を含む。` |
| examples.md:138 | `名前付きのレコード (S2) とコマンドリテラル (S3) を含む。` | `名前付きのレコード (M3) とコマンドリテラル (M9) を含む。` |
| examples.md:156 | `名前付きのレコード、リストのパターン、補間 (S2) を含む。` | `名前付きのレコード、リストのパターン、補間 (M3) を含む。` |
| declarations.md:64 | `` `type` と名前付きのレコードは S2 で実装する。`` | `` `type` と名前付きのレコードは M3 で実装する。`` |
| declarations.md:123 | `は、S2 で \`List\` のコンストラクタと一緒に` | `は、M3 で \`List\` のコンストラクタと一緒に` |
| declarations.md:133 | `将来の型クラス \`Eq\` の \`Int\`、\`String\`、\`Bool\` のインスタンスにあたる ([ロードマップ](../future/roadmap.md))` | `将来の型クラス \`Eq\` の \`Int\`、\`String\`、\`Bool\` のインスタンスにあたる ([ロードマップ](../future/roadmap.md) の「M4 数値と文字」と「M5 型クラス」)` |
| declarations.md:139 | `(S2 の \`Prelude\`)` | `(M3 で \`Prelude\` に入る)` |
| declarations.md:141 | `S2 で Prelude に入る` | `M3 で Prelude に入る` |
| declarations.md:149 | `モジュールと import は S2 で実装する。` | `モジュールと import は M2 で実装する。` |
| lexical.md:73 | `アドホック多相を入れるときに見直す ([ロードマップ](../future/roadmap.md))` | `M5 で穴を表示用のクラスに広げる ([ロードマップ](../future/roadmap.md) の「M5 型クラス」)` |
| lexical.md:77 | `S1 はエスケープだけの通常の文字列を扱う。補間、複数行の文字列、raw 文字列は S2 で実装し、` | `今の実装は、エスケープだけの通常の文字列を扱う。補間、複数行の文字列、raw 文字列は M3 で実装し、` |
| lexical.md:93 | `コマンドリテラルは S3 で、` | `コマンドリテラルは M9 で、` |
| modules.md:5 | `モジュールと import は S2 で実装する。` | `モジュールと import は M2 で実装する。` |
| core-ir.md:106 | `名前付きのレコードの実行時の表し方は、S2 で決める。` | `名前付きのレコードの実行時の表し方は、M3 で決める。` |
| grammar.md:116 | `文法はすべての段階の構文を含む。` | `文法はすべてのマイルストーンの構文を含む。` |
| grammar.md:116 | `S1 で実装した文法は次のとおり。` | `M1 で実装した文法は次のとおり。` |
| grammar.md:122 | 行全体 | `モジュールと import は M2 で、名前付きのレコードと \`type\`、リストのリテラル、補間、複数行の文字列、raw 文字列は M3 で、浮動小数と文字は M4 で、コマンドリテラルは M9 で実装する。マイルストーンの全体は [ロードマップ](../future/roadmap.md) の「マイルストーンの列」にある。` |
| grammar.md:124 | `S2 と S3 の構文は、パーサが CST まで組み、` | `まだ実装していない構文は、パーサが CST まで組み、` |
| grammar.md:126 | `S2 と S3 で lexer と一緒に作り直す。` | `M3 と M9 で lexer と一緒に作り直す。` |
| grammar.md:127 | `衝突を S2 で解くまで、` | `衝突を M3 で解くまで、` |
| diagnostics.md:51 | `射影と更新の番号は S2 で割り当てる。` | `射影と更新の番号は M3 で割り当てる。` |
| diagnostics.md:112 | `S2 と S3 の構文と、字句として予約した浮動小数と文字のリテラルである。` | `M2、M3、M9 の構文と、字句として予約した浮動小数と文字のリテラル (M4) である。` |
| expressions.md:187 | `(S2 で実装する)`、`(S3 で実装する)` | `(M3 で実装する)`、`(M9 で実装する)` |
| types.md:71 | `モジュール (S2) では` | `モジュール (M2) では` |
| records.md:75 | `row 多相の射影と一緒に S2 で決める` | `row 多相の射影と一緒に M3 で決める` |
| records.md:80 | `名前付きのレコードと \`type\` は S2 で実装する` | `名前付きのレコードと \`type\` は M3 で実装する` |
| records.md:82 | `名前付きのレコードと一緒に S2 で入れる` | `名前付きのレコードと一緒に M3 で入れる` |
| architecture.md:28 | `S2 で同じ名前の別の型の表示を決めるときに扱う ([status.md](status.md) の「S2 の材料」)。` | `M2 で同じ名前の別の型の表示を決めるときに扱う ([ロードマップ](../future/roadmap.md) の「M2 モジュール」)。` |
| architecture.md:120 | `これらを実装する S2 と S3 で行う` | `これらを実装する M3 と M9 で行う` |
| architecture.md:191 | `import をたどるローダは S2 で足す。` | `import をたどるローダは M2 で足す。` |
| testing.md:57 | `` `s1.em` は S1 の構文、`later_stages.em` は S2 以降の構文を含む `` | `` `s1.em` は M1 で実装した構文、`later_stages.em` はまだ実装していない構文を含む `` |

表の中の `\`` は、表の区切りのために逃がしたバッククォートで、実際の文字はバッククォート1つである。

- [ ] **Step 2: 取り残しを確かめる**

Run: `cd /Users/arakaki/Projects/eml && grep -rnE '\bS[123]\b' docs/spec docs/implementation`
Expected: 出力なし。

- [ ] **Step 3: コミットする**

```bash
cd /Users/arakaki/Projects/eml && git add docs/spec docs/implementation && git commit -q -m "Replace the syntax stage names S1 to S3 with milestones in the spec and implementation docs

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NQY7m9oreBjUG1cr17avCh"
```

---

### Task 6: crates のコメントの S2/S3 を置き換える

テストの期待値を変えない機械的な変更 (種類3) である。`crates/eml_syntax/tests/corpus.rs:48` と `crates/eml_hir/tests/def_map.rs:106` はテストのコードの中のコメントだが、期待値は変えない。

**Files:**
- Modify: `crates/eml_syntax/src/literal.rs:47`、`crates/eml_syntax/src/grammar/expressions.rs:261`、`crates/eml_syntax/src/grammar/items.rs:207,282`、`crates/eml_syntax/src/lexer/string.rs:1-2,85,119,140,163`、`crates/eml_syntax/src/grammar/mod.rs:233`、`crates/eml_syntax/src/syntax_kind.rs:17,169`、`crates/eml_cli/src/lib.rs:21,62`、`crates/eml_hir/src/lower/mod.rs:193`、`crates/eml_hir/src/lower/expr.rs:693,777`、`crates/eml_syntax/tests/corpus.rs:48`、`crates/eml_hir/tests/def_map.rs:106`

- [ ] **Step 1: 置き換える**

| 場所 | 今 | 新しく |
|---|---|---|
| literal.rs:47 | `補間を含むもの (S2)` | `補間を含むもの (M3)` |
| grammar/expressions.rs:261 | `中身の穴を S3 で lexer のモードと一緒に読むので` | `中身の穴を M9 で lexer のモードと一緒に読むので` |
| grammar/items.rs:207 | `/// S2 で実装する。CST まで組み、` | `/// M3 で実装する。CST まで組み、` |
| grammar/items.rs:282 | `/// import は S2 で実装する。` | `/// import は M2 で実装する。` |
| lexer/string.rs:1-2 | `補間、複数行の文字列、raw 文字列は S2、コマンド` と次の行の `リテラルは S3 で実装する。` | `補間、複数行の文字列、raw 文字列は M3、コマンド` と `リテラルは M9 で実装する。` |
| lexer/string.rs:85 | `/// 補間は S2 で実装する。` | `/// 補間は M3 で実装する。` |
| lexer/string.rs:119 | `/// S2 で実装する。今は閉じの \`"""\` までを` | `/// M3 で実装する。今は閉じの \`"""\` までを` |
| lexer/string.rs:140 | `/// S2 で実装する。今は閉じまでを1つのトークンにして` | `/// M3 で実装する。今は閉じまでを1つのトークンにして` |
| lexer/string.rs:163 | `/// S3 で実装する。` | `/// M9 で実装する。` |
| grammar/mod.rs:233 | `S2 の構文の中で診断を連鎖させないため。` | `M3 の構文の中で診断を連鎖させないため。` |
| syntax_kind.rs:17 | `部品に分けるのは補間を実装する S2 から。` | `部品に分けるのは補間を実装する M3 から。` |
| syntax_kind.rs:169 | `S2 で補間の \`\{\` と \`}\` を足すときも、` | `M3 で補間の \`\{\` と \`}\` を足すときも、` |
| eml_cli/src/lib.rs:21 | `import をたどるローダは S2 で足す。` | `import をたどるローダは M2 で足す。` |
| eml_cli/src/lib.rs:62 | `モジュール (S2) は` | `モジュール (M2) は` |
| lower/mod.rs:193 | `修飾名は S2 で実装する` | `修飾名は M2 で実装する` |
| lower/expr.rs:693 | `` `::` は S2 のリストのコンストラクタである。`` | `` `::` は M3 のリストのコンストラクタである。`` |
| lower/expr.rs:777 | `/// S2 で実装するリテラル。` | `/// M3 (複数行の文字列、raw 文字列) と M4 (浮動小数、文字) で実装するリテラル。` |
| corpus.rs:48 | `// S2・S3 の構文は E0004 だけを出し、` | `// まだ実装していない構文は E0004 だけを出し、` |
| def_map.rs:106 | `` `::` は S2 のリストのコンストラクタで、`` | `` `::` は M3 のリストのコンストラクタで、`` |

lower/expr.rs:777 は、置き換えた後に行が100字を大きく超えるなら、今の書き方に合わせて `///` の行を2行に分ける。

- [ ] **Step 2: 書式と取り残しを確かめる**

Run: `cd /Users/arakaki/Projects/eml && cargo fmt --check && grep -rnE '\bS[23]\b' crates`
Expected: `cargo fmt --check` は差分なし。grep は出力なし。

- [ ] **Step 3: テストを走らせる**

Run: `cd /Users/arakaki/Projects/eml && cargo test 2>&1 | grep -E "^test result" | awk '{s+=$4; f+=$6} END {print "passed", s, "failed", f}'`
Expected: `failed 0` で、passed の数が Task 0 の Step 3 と同じ。

- [ ] **Step 4: コミットする**

```bash
cd /Users/arakaki/Projects/eml && git add crates && git commit -q -m "Name milestones instead of syntax stages in code comments

Mechanical comment-only change (kind 3): no code or expected value changes.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NQY7m9oreBjUG1cr17avCh"
```

---

### Task 7: 全体を確かめる

**Files:**
- なし (確かめるだけ。見つけた問題は、その文書を受け持つタスクの形で直してコミットする)

- [ ] **Step 1: S1/S2/S3 の残りを確かめる**

Run: `cd /Users/arakaki/Projects/eml && grep -rnE '\bS[123]\b' docs CLAUDE.md crates --exclude-dir=superpowers`
Expected: 次の6行だけ。

```
crates/eml_syntax/tests/corpus.rs:5:const S1: &str = include_str!("corpus/s1.em");
crates/eml_syntax/tests/corpus.rs:10:    assert_eq!(diagnostics(S1), Vec::<String>::new());
crates/eml_syntax/tests/corpus.rs:16:        item_kinds(S1),
crates/eml_syntax/tests/corpus.rs:76:    for corpus in [S1, LATER_STAGES] {
crates/eml_syntax/tests/corpus.rs:87:    for corpus in [S1, LATER_STAGES] {
crates/eml_syntax/tests/corpus/s1.em:2:-- | S1 の構文をひととおり使うプログラム
```

- [ ] **Step 2: 節への参照を確かめる**

Run:
```bash
cd /Users/arakaki/Projects/eml && python3 /private/tmp/claude-501/-Users-arakaki-Projects-eml/54b9c476-d7b7-4ebd-ba96-b6efde49ca57/scratchpad/check_refs.py . > /private/tmp/claude-501/-Users-arakaki-Projects-eml/54b9c476-d7b7-4ebd-ba96-b6efde49ca57/scratchpad/refs_after.txt; diff <(sed -E 's/:[0-9]+:/:/' /private/tmp/claude-501/-Users-arakaki-Projects-eml/54b9c476-d7b7-4ebd-ba96-b6efde49ca57/scratchpad/refs_before.txt | sort) <(sed -E 's/:[0-9]+:/:/' /private/tmp/claude-501/-Users-arakaki-Projects-eml/54b9c476-d7b7-4ebd-ba96-b6efde49ca57/scratchpad/refs_after.txt | sort)
```
Expected: `>` で始まる行 (新しく切れた参照) がない。`<` の行 (直った参照や、消えた参照) はあってよい。新しい行があれば、引いている側か roadmap の見出しを直す。

- [ ] **Step 3: spec の「文書ごとの変更」を1行ずつ突き合わせる**

spec の「文書ごとの変更」の表の各行について、`git diff 52cc7ad -- <文書>` を読み、その行の変更が入っていることを確かめる。抜けがあれば、その文書を受け持つタスクの手順で直す。

- [ ] **Step 4: テストと書式を確かめる**

Run: `cd /Users/arakaki/Projects/eml && cargo fmt --check && cargo test 2>&1 | grep -E "^test result" | awk '{s+=$4; f+=$6} END {print "passed", s, "failed", f}'`
Expected: 書式の差分なし、`failed 0`、passed の数が Task 0 と同じ。

---

### Task 8: 作業用の文書を消す

利用者が組み直しの結果を確かめて了承した後に行う。

**Files:**
- Delete: `docs/superpowers/specs/2026-10-07-roadmap-restructure-design.md`、`docs/superpowers/plans/2026-10-07-roadmap-restructure.md`

- [ ] **Step 1: 消してコミットする**

```bash
cd /Users/arakaki/Projects/eml && git rm -q docs/superpowers/specs/2026-10-07-roadmap-restructure-design.md docs/superpowers/plans/2026-10-07-roadmap-restructure.md && git commit -q -m "Delete the roadmap restructure design and plan

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NQY7m9oreBjUG1cr17avCh"
```
