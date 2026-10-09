# ロードマップの引き直し 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** [spec](../specs/2026-10-10-roadmap-redraw-design.md) の段の列 S4〜S13 と各段の中身で、ロードマップと関連する文書を書き直す。

**Architecture:** 文書だけの作業である。コードはコメントの段の名前だけを変える。ロードマップの見出しを引く文書を、移した先の見出しに直し、`citations` のテストで確かめる。

**Tech Stack:** Markdown (日本語)、`cargo test` (引用の検査と、コメントだけの変更の確認)、yomiyasu のリンター。

**Spec:** `docs/superpowers/specs/2026-10-10-roadmap-redraw-design.md`

## Global Constraints

- 日本語の文章は、書く前に `yomiyasu:yomiyasu` のスキルを呼び、その規則に従う。文体は既存の文書と同じ常体 (である調) にする。
- `docs/` の引用は `[名前](path) の「見出し」` の形で書く。見出しを変えたら、引く側を直す (`docs/implementation/testing.md` の「文書の引用の検査」)。
- `spec/` と `implementation/` の文書は、段の名前と引用の行き先だけを直す。中身の説明は変えない。例外は、この計画が「書き換える」と明示した行だけである。
- テストの期待値はバイト単位で変えない (機械的な追随)。
- コミットのメッセージは英語で書き、最後に次の2行を付ける。

  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01QY7iQV7A3UZjiakV9Z8HHe
  ```

- `git diff` は外部の差分ツールを使う設定なので、スクリプトで差分を見るときは `git diff --no-ext-diff` を使う。

## Review Focus

- 移した先を取り違えた段の参照。たとえば、レコードの参照を S5 にしたり、`exit` の参照を S6 にしたりする誤り。下の対応表どおりであることを、grep の結果と1行ずつ照らす。
- 今のロードマップの「論点」や「決めたこと」が、新しいロードマップのどの節にも移らずに消えること。Task 1 の Step 6 で、今のロードマップの箇条書きを1つずつ新しい場所と照らす。
- 見出しは残っていて引用の検査は通るのに、引いている中身が別の節へ移ったもの (「処理系」の記述子の項目など)。Task 1 の Step 4 の対応表で直す。
- `spec/` や `implementation/` の規範の中身を、名前の置き換えのついでに書き換えてしまうこと。Task 3 の Step 5 で `git diff --no-ext-diff` を見て、対応表にない変更がないことを確かめる。
- `docs/reports/` の tar.gz をコミットしてしまうこと。Task 4 の Step 3 で `git status` を確かめる。

---

### Task 1: ロードマップを書き直す

**Files:**
- Modify: `docs/future/roadmap.md` (全体)
- Modify: ロードマップの見出しを引く文書 (Step 4 の表)
- Test: `crates/eml_cli/tests/citations.rs` (変えない。流すだけ)

**Interfaces:**
- Produces: 新しいロードマップの見出し。後の Task が引く。

  ```
  # ロードマップ
  ## 進め方
  ## 段の列
  ## S4 単相化と計測の基準
  ## S5 型クラス
  ## S6 リスト、文字列、レコード
  ## S7 evidence passing
  ## S8 LIR
  ## S9 共通ランタイム `eml_rt`
  ## S10 VM と切り替え
  ## S11 REPL
  ## S12 スクリプトの MVP
  ## S13 実例による判断
  ## S13 の後の言語の項目
  ### `Float`、`Char`、`Num`
  ### バイト列と `Array`
  ### UTF-8 と標準ライブラリ
  ### コマンドリテラル
  ## その後の項目
  ### 型システム
  ### 言語機能と構文
  ### 処理系
  ### マルチコア対応
  ```

  S4〜S12 の節は、それぞれ `### 決めたこと` と `### 論点` を持つ。S5 は、その後に `### 方式を選んだ理由` を持つ。

- [ ] **Step 1: 今のロードマップを写しておく**

  後の照合に使う。

  ```bash
  cp docs/future/roadmap.md "$TMPDIR/roadmap-old.md"
  ```

  `$TMPDIR` が使えない環境では、セッションの scratchpad のディレクトリに置く。

- [ ] **Step 2: yomiyasu を呼ぶ**

  `Skill` で `yomiyasu:yomiyasu` を呼ぶ。

- [ ] **Step 3: `docs/future/roadmap.md` を書き直す**

  上の見出しの構成で書く。各節の中身は次のとおりである。

  - **冒頭**
    - 「位置づけ: 将来の設計。」は残す。
    - 導入の段落は「再設計のサブプロジェクト S4〜S13 と、その後の言語の項目、処理系の項目に分けてまとめる」に変える。続く2文 (今の言語の範囲と overview へのリンク) は残す。
    - 次の1文を足す。「S4〜S13 の順序は、`docs/reports/eml/` の2本の調査レポート (VM とネイティブコード生成、REPL 向けのランタイム) を踏まえて決めた。」
  - **進め方**
    - 今の箇条書きをそのまま残す。
  - **段の列**
    - spec の「段の列」の表を写す。段、名前と中身の要約、前提、完了の条件の列にする。中身の要約は、spec の「各段の中身」から1行で書く。
    - 「S13 の後は、言語の項目を次の順に並べる」として、`Float`、`Char`、`Num` (前提 S5) → バイト列と `Array` (前提 S5、`Float`、`Char`、`Num`) → UTF-8 と標準ライブラリ → コマンドリテラル の表を置く。
    - 「処理系の項目は、言語の項目とは別の列にして、下の「その後の項目」に置く」の1文を残す。
    - spec の「順序の理由」の箇条書きを写す。
    - 今の「順序の理由」から、次の2つを書き直して残す。
      - 「`Float`、`Char`、`Num` を型クラスの後に置くのは、…」の型クラスを S5 にする。
      - 「バイト列と `Array` を `Num` の後に置くのは、…」はそのまま残す。
    - 今の「順序の理由」のほかの項目は、S12 の節 (線形型の負担をスクリプトで確かめる理由) と S5 の節 (参照ごとの具体化の表が土台になること) に移す。
  - **S4〜S12 の各節**
    - 「前提: …。」の1行を置いてから、`### 決めたこと` と `### 論点` を書く。
    - 中身は spec の「各段の中身」の該当する節から写す。spec が「今の … から移す」と書いた項目は、今のロードマップの文をそのまま移す。段の名前の参照は新しい名前に直す。
    - S5 には `### 方式を選んだ理由` を足す。中身は今の「方式を選んだ理由」の表と段落で、次のとおり書き直す。
      - 「Rust のトレイト + すべての総称の単相化」の行は、結論を次に変える。「採らない。データの配置まで型ごとに分けると、多相再帰と操作ごとに量化した型変数の節で一様なコードと配置が食い違い、言語を制限することになる。関数のコードの単相化は S4 で採る」
      - 採る行は「**型クラス + 関数のコードの単相化**」にする。結論は「**採る**。証拠は S4 の instance の表で確定する。データの配置は一様のままにする」とする。
      - 表の後の段落は、成り立つ条件 (rank-1、シグネチャが必須、局所の `let` とラムダが単相、局所的な instance と第一級の instance がない、操作とコンストラクタが制約を持たない) を残す。Roc と Austral の比較は「eml は関数のコードを単相化するが、データの配置は一様のまま残し、多相再帰と操作の節では一様な版を使う」に改める。代償は、コードの大きさ、REPL で新しい型の組ごとに変換が1つ増えること、の2つにする。
  - **S13 実例による判断**
    - 今の「S5 実例による判断」の本文を、前提を「S12」に変えてそのまま移す。
  - **S13 の後の言語の項目**
    - 今の4つの節を、次の変更だけで移す。
      - `Float`、`Char`、`Num`: 前提を「S5」に変える。「`Float` の Repr と、インタプリタの `Value::Float` を入れる」を「`Float` の Repr と、`eml_rt` の語での `Float` の表現を入れる」に変える。論点の「多相な位置での `Int` と `Float` のネイティブの表現は、下の「処理系」のネイティブ化で決める」を「多相な位置での `Float` の表現は、S9 の語の表現の上で決める」に変える。
      - バイト列と `Array`: 前提を「S5、`Float`、`Char`、`Num`」に変える。
      - UTF-8 と標準ライブラリ: 「S4 で Rust で実装した extern」を「S12 で Rust で実装した extern」に変える。
      - コマンドリテラル: 「`Proc.run` は S4 で extern として先に入っている」の S4 を S12 にする。
  - **その後の項目**
    - 導入の段落と「型システム」はそのまま移す。
    - 「言語機能と構文」もそのまま移し、前提だけ直す。フィールドの変換の糖衣の「前提: S4」は「前提: S6」に、接頭辞付きリテラルの「前提: S4、型クラス」は「前提: S5、S6」にする。
    - 「処理系」は spec の「「その後の項目」の変更」に従って直す。evidence passing の項目、操作の宣言した Repr の項目、記述子の項目は削る。ネイティブ化、Perceus の最適化、ループ化、jump threading とインライン化、実行時エラーの backtrace は書き直す。JIT の項目を足す。
    - 「マルチコア対応」はそのまま移し、「インタプリタはシングルスレッドにし、並列化はネイティブのランタイムだけで行う。インタプリタでは、…」を「VM はシングルスレッドにし、並列化はネイティブのランタイムだけで行う。VM では、…」に変える。

- [ ] **Step 4: ロードマップの見出しを引く文書を直す**

  見出しが残っていて引用の検査は通るものも、中身が別の節へ移ったなら行き先を直す。

  | ファイル:行 (着手時の行番号) | 今の引用 | 直した後 |
  |---|---|---|
  | `docs/spec/lexical.md:73` | 「S4 スクリプトの MVP」 | 書き換える。最後の文を「穴の型は、S6 で S5 の表示のクラスに広げる ([ロードマップ](../future/roadmap.md) の「S6 リスト、文字列、レコード」)」にする |
  | `docs/spec/declarations.md:154` | 「S4 スクリプトの MVP」 | 書き換える。「S4 の組み込みのクラス `Eq` の … インスタンスにあたる ([ロードマップ](../future/roadmap.md) の「S4 スクリプトの MVP」)」を「S5 の型クラス `Eq` の `Int`、`String`、`Bool` のインスタンスにあたる ([ロードマップ](../future/roadmap.md) の「S5 型クラス」)」にする |
  | `docs/future/stdlib.md:13` | 「S4 スクリプトの MVP」 | 「S12 スクリプトの MVP」 |
  | `docs/spec/core-ir.md:130` | 「REPL」 | 「S11 REPL」。文中の「REPL の段である」は「S11 である」にする |
  | `docs/spec/core-ir.md:112` | 「処理系」(`Int` の幅) | 文を「`tobj` の `Int` の表し方は S9 で決める ([ロードマップ](../future/roadmap.md) の「S9 共通ランタイム `eml_rt`」)」にする |
  | `docs/spec/core-ir.md:172` | 「処理系」(操作の Repr) | 「evidence passing と一緒に決める」を「S7 で決める」に、行き先を「S7 evidence passing」にする |
  | `docs/spec/runtime.md:20` | 「処理系」の記述子の項目 | 「形はネイティブ化の段階で決める」を「形は S9 で決める」に、行き先を「S9 共通ランタイム `eml_rt`」にする |
  | `docs/future/multicore.md:161` | 「処理系」の記述子の項目 | 行き先を「S9 共通ランタイム `eml_rt`」にする |
  | `docs/future/multicore.md:226` | 「処理系」 | Task 2 で直す |
  | `docs/future/evidence-passing.md:5`、`:18` | 「処理系」 | Task 2 で直す |
  | `docs/spec/core-ir.md:50`、`:327`、`docs/implementation/architecture.md:242`、`docs/implementation/status.md:32` | 「処理系」 | そのまま (借用、遅らせる形、jump threading、ネイティブ化は「処理系」に残る) |
  | `docs/spec/examples.md:5`、`docs/implementation/status.md:5` | 「段の列」 | そのまま (Task 3 で段の名前だけ直す) |
  | `docs/future/multicore.md:240`、`docs/spec/modules.md:128` | 「言語機能と構文」 | そのまま |
  | `docs/future/multicore.md:241`、`docs/future/stdlib.md:11` | 「バイト列と `Array`」 | そのまま |
  | `docs/future/multicore.md:244` | 「型システム」 | そのまま |
  | `docs/future/stdlib.md:18` | 「UTF-8 と標準ライブラリ」 | そのまま (Task 3 で S4 を S12 にする) |

  表の外に引用がないことを確かめる。

  ```bash
  grep -rnE 'roadmap\.md\)?( の|の)「' crates docs std tests CLAUDE.md | grep -v '^docs/superpowers\|^docs/reports\|^docs/future/roadmap.md'
  ```

  Expected: 上の表に挙げた行だけが出る。

- [ ] **Step 5: 引用の検査を流す**

  ```bash
  cargo test -p eml_cli --test integration citations::
  ```

  Expected: 2つのテスト (`every_cited_heading_exists`、`every_linked_document_exists`) が PASS する。FAIL したら、出力に出た引用を、Step 3 の見出しの一覧に合わせて直す。

- [ ] **Step 6: 移し漏れを照らす**

  `$TMPDIR/roadmap-old.md` の箇条書きと表の行を上から1つずつ読み、新しいロードマップのどこに移ったかを確かめる。spec が削ると決めたもの (evidence passing、操作の宣言した Repr、記述子の3項目) は、S7 と S9 の節に中身があることを確かめる。どこにもない項目があれば、spec の対応する段の節に足す。

- [ ] **Step 7: リンターを流す**

  ```bash
  python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.1.0/skills/yomiyasu/scripts/yomiyasu_lint.py docs/future/roadmap.md
  ```

  箇条書きの比率の警告は、今のロードマップと同じ形なので直さない。それ以外の指摘は、yomiyasu の Step 3 の基準で見直す。

- [ ] **Step 8: コミット**

  ```bash
  git add docs/future/roadmap.md docs/spec docs/future/stdlib.md docs/future/multicore.md
  git commit -m "Redraw the roadmap as stages S4-S13

  Monomorphization, type classes, data structures, evidence passing,
  LIR, the shared runtime, the VM and the REPL now come before the
  scripting MVP. Citations of moved roadmap headings follow them.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01QY7iQV7A3UZjiakV9Z8HHe"
  ```

### Task 2: evidence passing とマルチコアの設計を改める

**Files:**
- Modify: `docs/future/evidence-passing.md`
- Modify: `docs/future/multicore.md:226` 付近 (「インタプリタとネイティブのランタイム」の節) と、インタプリタに触れるほかの文

**Interfaces:**
- Consumes: Task 1 の見出し「S7 evidence passing」「S8 LIR」「S9 共通ランタイム `eml_rt`」「S10 VM と切り替え」

- [ ] **Step 1: yomiyasu を呼ぶ**

- [ ] **Step 2: `evidence-passing.md` を改める**

  - 冒頭の段落を「S7 ([ロードマップ](roadmap.md) の「S7 evidence passing」) で、エフェクトの操作と handler をどう実装するかの方針をまとめる。」に変える。予防的な決定の文は残す。
  - 「対象外」のレコードの項目は、行き先の節がないので、「row 多相なレコードは採らない ([ロードマップ](roadmap.md) の「S6 リスト、文字列、レコード」) ので、レコードの evidence passing は扱わない」に変える。
  - 「方式 A の概要」の前に「変換の位置と出力」の節を足す。中身は spec の S7 の次の項目である。
    - Core IR から Core IR への変換で、translate と box の挿入の間に置く。
    - 出力は Core IR の下位言語で、`handle` / `perform` / `resume` / `mask` を含まない。
    - evidence と yield の基本操作は、Repr の決まった表の形で持つ。
    - 出力は既存の命令の種類と基本操作だけにする。
    - verifier にこの下位言語の段を足す。
    - 下位言語は S8 で LIR に写す ([ロードマップ](roadmap.md) の「S8 LIR」)。
  - 「yield の bubbling」の節に、末尾の位置の扱い (縮約の定義に合わせ、末尾の位置の呼び出しの後に yield の確認を入れない) を足す。
  - 「Perceus と線形性との関係」の最初の項目を、「変換は box の挿入と Perceus の前に置く。S8 からは、box の挿入と Perceus は LIR の上のパスである」に変える。T3 と `never` の規則を EP の出力に合わせて書き直すことも、この節に足す。
  - 「実装に進むときの条件」の「差分テスト」を次に変える。
    - 開発中は、CEK に EP の基本操作を一時的に足し、変換の有無で UI テストの出力を比べる。RC の回数は比べない。
    - 段の終わりに EP を常に通し、CEK と各パスの、エフェクトを直接扱う部分を消す。
    - CEK そのものは、S10 で VM に置き換えて消す。
  - 「未決の論点」の「インタプリタの既定の実行を、変換後の IR に切り替えるか」は決まったので削る。`RunStats` の新しい指標の項目を足す。

- [ ] **Step 3: `multicore.md` を直す**

  ```bash
  grep -n 'インタプリタ\|eml_runtime\|検査付きヒープ' docs/future/multicore.md
  ```

  出た各行を次の方針で直す。

  - 「インタプリタ」が今の CEK の実行を指し、並列にしない側の実行系という意味なら、「VM」にする。
  - 「`eml_runtime` はインタプリタのための検査付きヒープ」とその周り (226行付近) は、「VM とネイティブは共通ランタイム `eml_rt` を使う。VM はシングルスレッドで、`eml_rt` の検査モードが今の検査付きヒープの役を引き継ぐ ([ロードマップ](roadmap.md) の「S9 共通ランタイム `eml_rt`」)。共有の印方式の RC は、`eml_rt` の RC の符号の規約 (負を複数スレッド用に予約) の上でネイティブのランタイムが使う」にする。
  - 引用の行き先は「処理系」から「S9 共通ランタイム `eml_rt`」に変える。

- [ ] **Step 4: 引用の検査とリンター**

  ```bash
  cargo test -p eml_cli --test integration citations::
  python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.1.0/skills/yomiyasu/scripts/yomiyasu_lint.py docs/future/evidence-passing.md
  python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.1.0/skills/yomiyasu/scripts/yomiyasu_lint.py docs/future/multicore.md
  ```

  Expected: 引用の検査が PASS する。リンターの指摘は、yomiyasu の Step 3 の基準で見直す。

- [ ] **Step 5: コミット**

  ```bash
  git add docs/future/evidence-passing.md docs/future/multicore.md
  git commit -m "Align the evidence passing and multicore designs with S7-S10

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01QY7iQV7A3UZjiakV9Z8HHe"
  ```

### Task 3: 段の名前の参照を直す

**Files:**
- Modify: `docs/overview.md`、`docs/README.md`、`docs/spec/{examples,lexical,declarations,modules,core-ir,records,expressions,diagnostics}.md`、`docs/implementation/{architecture,status,testing}.md`、`docs/future/stdlib.md`、`CLAUDE.md`、`docs/superpowers/specs/2026-10-07-redesign-design.md`
- Modify (コメントだけ): `crates/eml_syntax/src/syntax_kind.rs`、`crates/eml_syntax/src/literal.rs`、`crates/eml_syntax/src/grammar/mod.rs`、`crates/eml_syntax/src/grammar/items.rs`、`crates/eml_syntax/src/lexer/string.rs`、`crates/eml_hir/src/lower/expr.rs`、`crates/eml_hir/tests/def_map.rs`、`crates/eml_types/src/check/body.rs`、`crates/eml_types/tests/instantiations.rs`、`crates/eml_core_ir/tests/externs.rs`

**Interfaces:**
- Consumes: Task 1 の見出し

- [ ] **Step 1: yomiyasu を呼ぶ**

- [ ] **Step 2: 対応表のとおりに直す**

  行番号は着手時のものである。各行を開き、文脈を読んでから直す。「書き換える」と書いた行だけは文を変え、ほかは段の名前だけを変える。

  | ファイル:行 | 直し方 |
  |---|---|
  | `docs/overview.md:38` | 「(S4 で入れる)」→「(S6 で入れる)」 |
  | `docs/overview.md:39` | 書き換える。説明の列を「型クラス `Eq`、`Ord`、`Show` を Prelude に置く。`class` と `instance` の宣言、シグネチャの制約 `Eq a =>`、`deriving` を持つ。証拠は Core IR への変換の単相化で決め、実行時の辞書は持たない (S5 で入れる)」にする |
  | `docs/overview.md:61` | 「S5 で決める」→「S13 で決める」 |
  | `docs/overview.md:91` | 用語の行を「S0〜S13」にし、列挙の「S4 スクリプトの MVP、S5 実例による判断」を「S4 単相化と計測の基準、S5 型クラス、S6 リスト、文字列、レコード、S7 evidence passing、S8 LIR、S9 共通ランタイム、S10 VM と切り替え、S11 REPL、S12 スクリプトの MVP、S13 実例による判断」にする。行の残りは読んでから、段の名前だけ合わせる |
  | `docs/README.md:32` | 「再設計の段 (S4〜S5)」→「再設計の段 (S4〜S13)」 |
  | `docs/spec/examples.md:5` | 「S4 で、実際に動くスクリプトを」→「S12 で、実際に動くスクリプトを」 |
  | `docs/spec/examples.md:9`、`:69`、`:107`、`:138`、`:156` | 「(S4)」→「(S6)」 |
  | `docs/spec/lexical.md:73` | Task 1 で済み |
  | `docs/spec/lexical.md:77` | 「S4 で実装し」→「S6 で実装し」 |
  | `docs/spec/lexical.md:93` | 「S5 の後の「コマンドリテラル」の段」→「S13 の後の「コマンドリテラル」の段」 |
  | `docs/spec/declarations.md:64` | 「S4 で実装する」→「S6 で実装する」 |
  | `docs/spec/declarations.md:144`、`:160`、`:162` | 「S4」→「S6」 |
  | `docs/spec/declarations.md:154` | Task 1 で済み |
  | `docs/spec/modules.md:14` | 「S4 で入る」→「S6 で入る」 |
  | `docs/spec/modules.md:48` | 「型クラスの段の orphan 規則」→「S5 (型クラス) の orphan 規則」 |
  | `docs/spec/core-ir.md:131` | 文中の S4 (std の extern が名前的なデータを作る話) を S12 にする |
  | `docs/spec/core-ir.md:132` | 「S4 で」→「S6 で」 |
  | `docs/spec/core-ir.md:159` | 「S4 のローカルの関数」→「S12 のローカルの関数」 |
  | `docs/spec/core-ir.md:420` | 「名前付きのレコードの実行時の表し方は、S4 で決める」→「名前付きのレコードの実行時の表し方は、S9 で決める」 |
  | `docs/spec/records.md:75` | 「S4 で名前的なレコードと一緒に決める」→「S9 で決める」 |
  | `docs/spec/expressions.md:187` | 「S4 で実装する」→「S6 で実装する」 |
  | `docs/spec/diagnostics.md:120` | 「S4 とコマンドリテラルの段で入れる構文」→「S6 とコマンドリテラルの段で入れる構文」 |
  | `docs/implementation/architecture.md:149` | 「S4」→「S6」 |
  | `docs/implementation/architecture.md:187` | 「S4 の組み込みのクラスの証拠と、後の型クラスと `Num` の解決は」→「S4 の単相化、S5 の型クラスの証拠、後の `Num` の解決は」 |
  | `docs/implementation/architecture.md:188` | 「S4 と型クラスの段でも」→「S5 でも」 |
  | `docs/implementation/architecture.md:192` | 「S4 で特殊化のときに」→「S4 で単相化のときに」 |
  | `docs/implementation/status.md:5` | 「再設計 (S0〜S5)」→「再設計 (S0〜S13)」 |
  | `docs/implementation/status.md:27`、`:41`、`:42`、`:47` | 「S4」→「S6」 |
  | `docs/implementation/status.md:28` | 「(S4 の組み込みのクラスと、その後の型クラスの段)」→「(S5)」 |
  | `docs/implementation/status.md:29` | 「REPL (REPL の段)」→「REPL (S11)」 |
  | `docs/implementation/status.md:30` | 「(S4、UTF-8 と標準ライブラリの段)」→「(S12、UTF-8 と標準ライブラリの段)」。「extern の型とエフェクトの型引数も S4 で入れる」→「S12 で入れる」 |
  | `docs/implementation/testing.md:58` | 行を読み、S4 が多相な extern の比べ方の話なら S12 にする |
  | `docs/future/stdlib.md:14`、`:18` | 「S4」→「S12」 |
  | `docs/future/stdlib.md:15` | 「S4 で組み込みのクラス `Eq`、`Ord`、`Show` が入り」→「S5 で型クラス `Eq`、`Ord`、`Show` が入り」 |
  | `docs/future/stdlib.md:17` | 「穴を `Show` に広げるかは S4 で決める」→「穴の型は、S6 で S5 の表示のクラスに広げる」 |
  | `CLAUDE.md:60` | 「(records, lists and string interpolation in S4; float and char literals and command literals after S5)」→「(records, lists and string interpolation in S6; float and char literals and command literals after S13)」 |
  | `crates/eml_syntax/src/syntax_kind.rs:17`、`:171` | コメントの「S4」→「S6」 |
  | `crates/eml_syntax/src/literal.rs:47` | コメントの「S4」→「S6」 |
  | `crates/eml_syntax/src/grammar/mod.rs:240` | コメントの「S4 の構文」→「S6 の構文」 |
  | `crates/eml_syntax/src/grammar/items.rs:275` | コメントの「S4」→「S6」 |
  | `crates/eml_syntax/src/lexer/string.rs:1`、`:85`、`:119`、`:140` | コメントの「S4」→「S6」 |
  | `crates/eml_hir/src/lower/expr.rs:720`、`:808` | コメントの「S4」→「S6」 |
  | `crates/eml_hir/tests/def_map.rs:146` | コメントの「S4」→「S6」 |
  | `crates/eml_types/src/check/body.rs:448` | コメントを読み、「S4 の組み込みのクラス」を「S4 の単相化と S5 の型クラス」にする |
  | `crates/eml_types/tests/instantiations.rs:113` | そのまま (節の型変数の区別は S4 の単相化で決めるので、S4 のままで正しい) |
  | `crates/eml_core_ir/tests/externs.rs:131` | コメントの「多相な extern は S4 で入り」→「多相な extern は S12 で入り」 |

- [ ] **Step 3: 全体設計の順序の節を改める**

  `docs/superpowers/specs/2026-10-07-redesign-design.md` の「サブプロジェクトの順序と完了の条件」の節の先頭に、次の段落を足す。節の残りは、S0〜S3 の記録として残す。

  > 2026-10-10 に、S4 以降の段を S4〜S13 に引き直した。今の順序と各段の中身は [ロードマップ](../../future/roadmap.md) の「段の列」にある。この節の S4 と S5 の記述は、引き直す前のものである。

- [ ] **Step 4: 漏れを探す**

  ```bash
  grep -rnE 'S[45]([^0-9a-z]|$)|型クラスの段|REPL の段' docs CLAUDE.md crates std | grep -v '^docs/reports\|^docs/superpowers'
  ```

  Expected: 残るのは次の4種類だけである。
  - 新しいロードマップの中の S4 / S5 (新しい意味で使っているもの)。
  - `crates/eml_types/tests/instantiations.rs:113`。
  - `docs/spec/declarations.md` と `docs/implementation/architecture.md` で、S4 を単相化、S5 を型クラスの意味で使っている行。
  - `docs/overview.md:91` の列挙。

  ほかに出た行は、Step 2 の表の方針で直す。

- [ ] **Step 5: 規範の中身を変えていないことを確かめる**

  ```bash
  git diff --no-ext-diff --stat
  git diff --no-ext-diff -- docs/spec docs/implementation
  ```

  各変更が、Step 2 の表の行だけであることを読んで確かめる。

- [ ] **Step 6: テストを流す**

  ```bash
  cargo test
  ```

  Expected: すべて PASS する (コメントだけの変更なので、期待値は変わらない)。

- [ ] **Step 7: コミット**

  ```bash
  git add docs CLAUDE.md crates
  git commit -m "Rename stage references for the redrawn roadmap

  References to the old S4 now point to S5 (type classes), S6 (lists,
  strings, records) or S12 (scripting MVP); the old S5 is S13.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01QY7iQV7A3UZjiakV9Z8HHe"
  ```

### Task 4: レポートをコミットする

**Files:**
- Create: `docs/reports/eml/…` (md と小さい実験ファイル)
- Modify: `.gitignore`
- Modify: `docs/README.md` (文書の地図)

- [ ] **Step 1: tar.gz を無視する**

  `.gitignore` の末尾に次の行を足す。

  ```
  # 調査レポートに添付された、調査時のコードの写し
  docs/reports/**/*.tar.gz
  ```

- [ ] **Step 2: 文書の地図に行を足す**

  `docs/README.md` の文書の地図の表の最後 (`future/evidence-passing.md` の行の後) に、次の2行を足す。

  ```markdown
  | **reports/** | 調査 | 実装の方針を決めるための調査レポート。書いた時点のコミットに基づき、後から更新しない |
  | [reports/eml/](reports/eml/) | 調査 | VM とネイティブコード生成 (2026-10-09)、REPL 向けのランタイム (2026-10-09) |
  ```

  ディレクトリへのリンクが引用の検査で通らなければ、2行目をリンクなしの `reports/eml/` にする。

- [ ] **Step 3: コミットする**

  ```bash
  git add .gitignore docs/README.md docs/reports
  git status --short
  ```

  Expected: `docs/reports/` の下では、`report.md`、`README.md`、`results.md`、`*.em`、`*.diff`、`dump_example.rs` だけが追加になり、`.tar.gz` は出ない。

  ```bash
  cargo test -p eml_cli --test integration citations::
  git commit -m "Add the VM and runtime research reports

  The attached tarballs of the code at the time are ignored.

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01QY7iQV7A3UZjiakV9Z8HHe"
  ```

### Task 5: 仕上げ

**Files:**
- Delete: `docs/superpowers/specs/2026-10-10-roadmap-redraw-design.md`、`docs/superpowers/plans/2026-10-10-roadmap-redraw.md`
- Modify: メモリ `/Users/arakaki/.claude/projects/-Users-arakaki-Projects-eml/memory/redesign-s0-s5.md` と `MEMORY.md`

- [ ] **Step 1: 全体のテスト**

  ```bash
  cargo test
  cargo clippy --all-targets
  ```

  Expected: すべて PASS し、警告がない。

- [ ] **Step 2: spec の決定が文書に入ったことを照らす**

  spec の D1〜D10 と「各段の中身」の各項目について、新しいロードマップか `evidence-passing.md` のどこに書いたかを1つずつ確かめる。抜けがあれば Task 1 か Task 2 の文書に足して、そのファイルだけをコミットする。

- [ ] **Step 3: この作業の spec と計画を消す**

  ```bash
  git rm docs/superpowers/specs/2026-10-10-roadmap-redraw-design.md docs/superpowers/plans/2026-10-10-roadmap-redraw.md
  git commit -m "Delete the roadmap redraw design and plan

  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01QY7iQV7A3UZjiakV9Z8HHe"
  ```

- [ ] **Step 4: メモリを更新する**

  `redesign-s0-s5.md` の description を「eml is mid aggressive redesign; S0-S3b-2c-2 done; S4-S13 re-planned 2026-10-10 (mono, type classes, data, EP, LIR, eml_rt, VM, REPL before the scripting MVP); order lives in docs/future/roadmap.md」に変える。本文の最後に、次の1段落を足す。

  > 2026-10-10 に、ユーザーと roadmap を S4〜S13 に引き直した。
  > - 段の順: S4 単相化 → S5 型クラス → S6 リスト/文字列/レコード → S7 EP → S8 LIR → S9 eml_rt → S10 VM → S11 REPL → S12 スクリプトの MVP → S13 実例による判断。
  > - 関数のコードは単相化し、データの配置は一様のまま残す。
  > - CEK は S10 で消す。エフェクトを直接扱う部分は S7 の終わりに消す。
  > - LIR は EP の直後に切る。
  > - `eml_rt` は語の表現で、`unsafe` の核と検査モードを持つ。
  > - reuse は後に回すが、その API は S9 で入れる。

  `MEMORY.md` の該当する行の説明も合わせる。
