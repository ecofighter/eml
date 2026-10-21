# 文書を今の状態の記述に整え、spec と実装の食い違いを直す計画

- 作成: 2026-10-21 (main の 0fff343a の時点)
- 状態: 計画は確定。実行の途中でユーザーに聞く項目が1つある (U-1)

この計画は `plan.md` と5つの付録からなる。付録は設計の作業の成果物をそのまま収めたもので、行番号、文案、表、コマンドの細部を持つ。`plan.md` と付録が食い違うときは `plan.md` に従う。付録の中の S1〜S4 は設計の作業の名前で、次の付録に当たる。

| 名前 | 付録 | 中身 |
|---|---|---|
| S1 | [a-rules.md](a-rules.md) | 文書の役割、経緯の扱い、benchmarks の形、段の進め方と閉じる手順、引用の方針、決まりの文案 |
| S2 | [b-status-roadmap.md](b-status-roadmap.md) | status.md と roadmap.md の章立て、項目ごとの行き先、移す先に書く事実、付け替えの表、W1〜W3 |
| S3 | [c-benchmarks.md](c-benchmarks.md) | benchmarks.md の章立て、所見、引用の付け替えの表、コードの段の名前、測り直し、D、A、B1〜B3、C2、M、C1 |
| S4 | [d-spec-fixes.md](d-spec-fixes.md) | spec の食い違いの判定の表、W-C1、W-C2、W-D1〜W-D17 (W-D12 は末尾の置き換えが正) |
| S4 の追加 | [e-history.md](e-history.md) | docs/ の経緯の記述の除去、W-H1〜W-H16 |

## 1. 目的と完了の条件

- 目的
  - `docs/` の文書を、段の経緯でなく今の状態の記述にする。status.md は今の状態、roadmap.md は今の状態に対する将来の論点、benchmarks.md は基準のプログラムと測り方と今の数値と所見を書く
  - 段の作業文書 (`docs/superpowers/`) を消し、今後の段も閉じるときに作業文書を消す決まりにする
  - spec と、コードやほかの文書の食い違いを直す
- 完了の条件 (8章の全体の確かめが、計画書を消す前にすべて通る)

## 2. 範囲

- 範囲に入るもの: `docs/` の全文書 (`docs/reports/` を除く)、`CLAUDE.md`、`crates/eml_cli/tests/citations.rs`、コードとテストと `bench/` のコメントの引用と段の名前、`eml_core_ir` の公開名 `type_repr`、`crates/eml_vm/tests/bench.rs` の定数、`tests/ui/check-fail/` の68本の移動、benchmarks.md の数値の測り直し
- 範囲に入らないもの
  - `docs/reports/` (書いた時点のまま残す)
  - コードの振る舞いの変更。spec と食い違ったコードは、spec を直す側に判定した (付録 D)。例外は W-C1 の公開名の改名だけである
  - E0004 のラベルの文字列 "this is implemented in a later stage" など、段の名前の検索に当たらないコードの文言
  - S14e の最終レビューの Minor 1 と Minor 6 に当たるコミットがないこと (レビューの原文がリポジトリになく、欠番か未対応かを区別できない)。計画の外で、ユーザーに伝えるだけにする

## 3. 背景

- S14 は閉じている。`cargo test --workspace` は全部通り、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、features を変えた clippy、`cargo verus verify -p eml_rt`、`verusfmt --check` も通る (0fff343a で確かめた。Miri は流していない)
- `crates/eml_cli/tests/citations.rs` は、`docs/…md の「見出し」` と `[…](…md) の「見出し」` の形の引用について、見出しが行き先にあるかを確かめる。走査するのは `crates`、`docs`、`tests`、`std`、根の `CLAUDE.md` と `README.md` で、`docs/superpowers` と `target` を除く (`citations.rs:15-34`)。見出しの完全一致を見るので、見出しを消すか改名するコミットは、引く側も同じコミットで直す
- この計画のファイルを `docs/plans/` に置いたままにすると、`citations::` の2つのテストが落ちる。付録が、まだない見出しを引くためである (計画を書いた時点で確かめた)。T-0 で `docs/plans` を走査から外す
- benchmarks.md の外から benchmarks.md を引く箇所は103 (うち `bench/` が26)、docs/superpowers を引く箇所は superpowers の外に10行ある (付録 C の 1.4)
- benchmarks.md の今の最新の表は `1a91f47-dirty` で測ったもので、`bench/rt.sh` の表は S14 の段で流していない (付録 C)

## 4. 決定

### 4.1 長期の決まり (変えるなら設計からやり直す)

実装の途中でこの決まりと食い違う所が出たら、止めてユーザーに報告する。中身と文案は付録 A の1章と5章にある。

#### D-1: 文書の役割
- 決定: 付録 A の「問い1」の表のとおり。spec は今の規則とその理由、status.md は `.em` を書く人から見た今の範囲と既知の制限、architecture.md は処理系の内部の構成、benchmarks.md は計測、roadmap.md は進め方と今後の項目を持つ。数値は benchmarks.md だけが持つ (利用者が当たる上限の値は status.md に数値で書いてよい)
- 影響の範囲: すべての文書

#### D-2: 経緯の扱い
- 決定: `docs/` は今の状態を書く。段の名前、日付、前の状態との対比、決定の記録 (誰がいつ決めたか) は書かず、git の履歴と閉じるコミットのメッセージに任せる。規則の理由と採らなかった案は「〜にすると〜になるため採らない」の形で規則の隣に残す。種類ごとの扱いは付録 A の「問い2」の表のとおり。例外は `docs/reports/` で、コミットのハッシュを書くのは benchmarks.md だけである
- 影響の範囲: `docs/` の全文書、コードのコメント

#### D-3: benchmarks.md の形と表の入れ替え
- 決定: 付録 A の「問い3」のとおり。段ごとの記録の節は持たない。「今の数値」の3つの表 (`bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh`) は、段を閉じるときに、きれいな作業ツリーで測り直して入れ替える。`-dirty` の値は載せない。大きさを変えるときは閉じるコミットの前の別のコミットにする。「所見」は題名の見出しで、今も判断の根拠になっている計測と、受け入れた後退とその理由だけを残す

#### D-4: 段の進め方と作業文書
- 決定: 付録 A の「問い4」のとおり。段の設計と計画は `docs/plans/YYYY-MM-DD-<段>.md` に置き、承認した版を段の最初のコミットで main に入れ、段を閉じるコミットで消す。spec は規則を実装するコミットと同じコミットで書く。閉じる手順は roadmap.md の「進め方」の文案 (付録 A の5章) に書く

#### D-5: 引用の方針
- 決定: 付録 A の「問い5」のとおり。引くのは題名の見出しだけで、段の名前の見出しを作らない。見出しを引く文は、その見出しがあるコミットに入れる。`docs/plans` と `docs/reports` の見出しは引かない。`citations.rs` は `bench/` も走査し、`docs/reports` への見出しつきの引用を誤りにする

#### D-6: 決まりを書く場所
- 決定: 付録 A の「問い6」の表と5章の文案のとおり (CLAUDE.md、docs/README.md の「文書の扱い」と地図、roadmap.md の「進め方」、testing.md の2節、benchmarks.md の冒頭)

### 4.2 短中期の設計 (理由があれば実装の中で調整してよい。調整はコミットのメッセージに書く)

#### D-10: status.md と roadmap.md の書き直し
- 決定: 付録 B のとおり。status.md の「深さと性能」は「コンパイルの時間と深さ」「実行の費用」「最適化が扱わない形」に分ける。処理系の内部の節は spec と architecture.md に移す (「未対応の構文と E0004」は spec/diagnostics.md に同じ名前の節を作る)。roadmap.md の見出しは消さず、改名もしない。実装済みの項目は今の状態の書き方にし、jump threading の規則は core-ir.md に移す
- 所見の見出しの読み替え: 付録 B の 1.6 は S3 の初版の見出しで書いてある。次の表で読み替え、付録 C の見出しを使う

| 付録 B の見出し | 使う所 | 付録 C の見出し |
|---|---|---|
| 「末尾でない再帰のフレームの大きさ」 | status.md の 101 (インライン化) | 「インライン化で増えるスタック」 |
| 「末尾でない再帰のフレームの大きさ」 | status.md の 103 (reuse の token) | 「reuse の token で増えるスタック」 |
| 「配列の要素の箱」 | status.md の 110、roadmap.md の「構造体のフィールドを親の物体に並べること」 | 「配列に書く構造体の箱」 |
| 「配列の要素の箱」 | roadmap.md の「スカラーの要素を生で持つ配列」 | 「配列の `Float` の要素の箱」 |
| ほかの見出し | | 付録 B と同じ名前 |

#### D-11: benchmarks.md の書き直し
- 決定: 付録 C のとおり。ただし所見は19にする。付録 C の所見9「ブロック引数の合併と比較の融合」は作らない (D-14 の2)。見出しの番号は付録の中の整理のためのもので、文書には番号を書かない
- `bench/scripts.sh` の fannkuch-redux の行は、実時間が範囲を外れても大きさを変えない (付録 C の M)

#### D-12: spec の食い違いの直し
- 決定: 付録 D の判定の表のとおり。食い違いはほとんど spec の側を直す。コードを直すのは W-C1 (`eml_core_ir::type_repr` の公開名を `boxed_repr` にする) と W-C2 (bench.rs の `EP_FLOAT_CONVERSIONS` の特別扱いを消す) だけである。W-D12 は付録 D の末尾の「W-D12 (置き換え)」に従い、check-fail/ の68本を番号の範囲の分類へ移す

#### D-13: docs/ の経緯の記述の除去
- 決定: 付録 E のとおり。規則 R1〜R8 で機械的に直せる行は規則で直し、判断の分かれる行は表の直し方に従う。S3 の B2 と S2 が直す文には触れない (付録 E の0章)

#### D-14: ユーザーが決めたこと (2026-10-21)
1. check-fail/ の機能の分類にある68本は、testing.md の今の規則 (番号の範囲で分ける) に合わせて移す。付録 D の W-D12 (置き換え) が手順を持つ
2. vm.md の CEK のインタプリタとの比べは消す。所見を作らない。vm.md:130 と :250 は「簡潔さを先にした」で止めて引用を外し、:411 は「前のインタプリタの数え方の値」と書いて引用を外す (付録 C の6章の Q1 の (c)。この行は B2 が直す)
3. roadmap.md の「後に回した理由」と「見直す条件」のうち、出典のない7つの欄は、T-9 を実行するときにユーザーに聞く (U-1)
4. 次は既定で進める: 単独の `\r` を「空白だが改行ではない」と lexical.md に書く。CLAUDE.md の E0004 の理由の句を "means the same at every stage" にする。bench.rs の `EP_FLOAT_CONVERSIONS` と分岐を消す。`SPECTRAL_NORM_S14E1_ALLOCATIONS` を `SPECTRAL_NORM_BOXED_TUPLE_ALLOCATIONS` に改名する。roadmap.md の「CPS のラムダの位置の変換 (試作で 200,125 回)」の文を消す。status.md の spec と重なる項目は、引用と段の名前と数値の直しだけにとどめる。「`var` の添字の費用」は所見にしない

#### D-15: 作業どうしのつなぎ
- `docs/plans` の除外は T-0 で足す。`docs/superpowers` の除外は T-1 では残し、T-2 で `docs/superpowers` を消すコミットで外す (付録 A の作業の単位1は「付け替える」と書くが、T-1 の時点で superpowers の除外を外すと superpowers の引用が検査に入って落ちるため)
- 付録 A の作業の単位1は、benchmarks.md に「今の数値」と空の「所見」の節を作り、今ある最新の表を写す。写した表は `-dirty` を含むが、T-17 で測り直して入れ替える
- 付録 A の作業の単位4は「S4 が docs/superpowers を消す」と書くが、消すのは T-2 (S3 の D) である
- 付録 B の作業の順は「S1 の単位1 → W1 → S3 の A → W2 → W3」で、付録 C、D、E の順と合わせた全体の順は7章のとおりである。付録の中の「前の値」は、それぞれの付録が前提にした順で数えてある。7章の順で付録の前提と違う所 (付録 B の W1 は S3 の A の後になる) では、前の値が付録の値と違ってもよく、後の値が合えばよい
- spec/diagnostics.md の「未対応の構文と E0004」の段落は T-7 (S2 の W1) が移して書き、T-14 (W-H1) はその文案の3つの語句だけを直す (付録 E の0章)

## 5. 実装の制約と決まり

- 日本語の文章 (docs/ の文書とコードのコメント) は、書く前に `yomiyasu:yomiyasu` スキルを呼び、その規則に従う (CLAUDE.md の規則)
- コードのコメントは why を書き、docs/ の節を引く。引くのは題名の見出しだけにする (D-5)
- 見出しを消すか改名するコミットは、引く側を同じコミットで直す (D-5)
- docs/ に段の名前、日付、前の状態との対比を書かない (D-2)。移行の途中の状態は計画書とコミットのメッセージにだけ書く
- テストの変更の種類 (testing.md の「テストの変更の運用」): この計画の承認がテストの変更の合意である。成否の変更はない。期待値の変更は T-13 の W-D12 (check-fail/ の68本の移動と、スナップショットの中のパスの行) だけである。ほかのテストの変更 (コメント、定数の改名、W-C1 の名前、W-C2 の分岐の削除) は期待値をバイト単位で変えない機械的な追随で、コミットのメッセージに書く
- どのコミットでも次が通る: `cargo test -p eml_cli --test integration citations::`。コードかテストを変えるコミットでは、加えて `cargo test`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` が通る。`crates/eml_rt` を変えるコミットでは、`crates/eml_rt/CLAUDE.md` が定める追加の検査 (`cargo verus verify -p eml_rt`、`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`) も通る
- 付録のコマンドが使う作業ディレクトリ `$S` (付録 C) はリポジトリの外に置く (T-17 の前提の `git status --porcelain` を空に保つため)。付録 C の `record_cites.py` と `args_only.em` は付録 C の7章の中身で `$S` に置く
- 作業は main に直接コミットする
- T-17 (測り直し) の後には、`crates/`、`std/`、`bench/` を変えるコミットを入れない (D-3)

## 6. 実装の作業

| ID | 作業 | 付録の作業 | 決定 | 依存 |
|---|---|---|---|---|
| T-0 | 計画書を入れ、`docs/plans` を検査から外す | なし | D-4、D-15 | - |
| T-1 | 決まりの文書化と検査の整え | 付録 A の作業の単位1 | D-1〜D-6 | T-0 |
| T-2 | docs/superpowers を消す | 付録 C の D | D-2、D-15 | T-1 |
| T-3 | 所見の見出しを足す | 付録 C の A | D-3、D-11 | T-1 |
| T-4 | コードとテストと bench/ の引用、eml_rt の段の名前 | 付録 C の B1 | D-5、D-11 | T-3 |
| T-5 | spec の記録への引用の付け替え | 付録 C の B2 | D-5、D-14 | T-3 |
| T-6 | コードとテストの残りの段の名前 | 付録 C の B3 | D-2 | T-1 |
| T-7 | 処理系の内部の節を status.md から外す | 付録 B の W1 | D-1、D-10 | T-1 |
| T-8 | status.md を書き直す | 付録 B の W2 | D-10 | T-3、T-7 |
| T-9 | roadmap.md を書き直す | 付録 B の W3 | D-10、D-14 | T-8、U-1 |
| T-10 | `boxed_repr` の公開名 | 付録 D の W-C1 | D-12 | T-1 |
| T-11 | bench.rs の `conversions` の検査を1つにする | 付録 D の W-C2 | D-12、D-14 | T-4 |
| T-12 | spec と手引きの食い違いの直し (W-D12 を除く16コミット) | 付録 D の W-D1〜W-D11、W-D13〜W-D17 | D-12 | T-5、T-9、T-10 |
| T-13 | check-fail/ の68本の移動と testing.md の分類 | 付録 D の W-D12 (置き換え) | D-12、D-14 | T-12 の W-D11 |
| T-14 | docs/ の経緯の記述の除去 (16コミット) | 付録 E の W-H1〜W-H16 | D-2、D-13 | T-12、T-13 |
| T-15 | 記録の節を消す | 付録 C の C2 | D-3 | T-4、T-5、T-8、T-9、T-14 |
| T-16 | 大きさの変更 (要るときだけ) | 付録 C の M の手順2 | D-3 | T-15 |
| T-17 | 測り直しと、今の数値と本文の書き直し | 付録 C の M と C1 | D-3、D-11 | T-16 |
| T-18 | 全体の確かめ (コミットなし) | なし | 全部 | T-17 |

### T-0: 計画書を入れる
- 目的: 計画書を main に入れても `citations::` が通るようにする
- 対象: `docs/plans/2026-10-21-docs-current-state/` と `crates/eml_cli/tests/citations.rs`
- 作業: `sources` の除外の条件 (`citations.rs:25` の `dir.ends_with("docs/superpowers")`) に `docs/plans` を足し、`citations.rs:15` の doc コメントを「`docs/superpowers` と `docs/plans` は作業中の設計と計画で、まだない節を引くので除く」の趣旨に直す。計画書と同じコミットにする
- 完了の条件: `cargo test -p eml_cli --test integration citations::` が3件とも通る (T-0 の前は、計画書があると2件落ちる)

### T-1 から T-17
- 作業、対象、手順、完了の条件、テストの変更の種類は、表の「付録の作業」の節にある。付録の手順のうち、この計画の決定 (D-14、D-15) と食い違う所は決定に従う
- T-1: 付録 A の5章の文案で、CLAUDE.md、docs/README.md、roadmap.md の「進め方」、testing.md の2節、benchmarks.md の冒頭を置き換える。`citations.rs` は `bench/` を走査に加え (拡張子 `py`、`sh` と、コメント記号 `#`)、`docs/reports` への見出しつきの引用を誤りにするテストを足す。`docs/superpowers` の除外は残す (D-15)
- T-2: 付録 C の D に加えて、`citations.rs` の `docs/superpowers` の除外を外し、T-0 で直した doc コメントから `docs/superpowers` を消す。testing.md の「文書の引用の検査」の superpowers の文は T-1 の文案で消えている
- T-3: 所見は19 (D-11)。付録 C の A の確かめの「0 から 20」は「0 から 19」と読む
- T-5: vm.md の3行は D-14 の2のとおりに直す
- T-8 と T-9: 所見への引用は D-10 の表で読み替える。status.md:96 と roadmap.md:28 の「約20倍」「約1.7秒」は消し、「1バイトずつ回すループ」を引く (付録 C の 1.3.1)
- T-9: 書き始める前に U-1 の7つの欄をユーザーに聞く。答えのあった欄だけを書く。付録 B の W3 の確かめの「後に回した理由」(後 6) と「見直す条件」(後 3) は、答えた欄の数だけ増える
- T-12: 付録 D の作業の列の順に、W-D12 を飛ばして1つずつコミットする
- T-13: 付録 D の W-D12 (置き換え) の2つのコミット。1つ目 (移動) が期待値の変更で、範囲は「check-fail/ の機能の分類の68本と、その68のスナップショットの中のパスの行」である。コミットのメッセージに種類と理由 (testing.md の番号の範囲の規則に合わせる) を書く
- T-14: 付録 E の順に1つずつコミットする
- T-16: 付録 C の M の手順2で範囲を外れた行があるときだけ、`Resize bench rows` のコミットを作る
- T-17: 付録 C の M の手順1〜7を流し (コミットなし)、C1 のコミットを作る。M の手順7 で `codegen-units` の所見が成り立たなかったら (どの行も同じ向きに変わったら)、C1 に進まずユーザーに聞く (U-2)

### T-18: 全体の確かめ
- 目的: 1章の完了の条件を確かめる
- 作業: 8章のコマンドを流す。当たりが残ったら、その文書の作業に戻って直し、直したコミットの後でもう一度流す。T-17 の後なので、直すのは文書だけで、`crates/`、`std/`、`bench/` を変える直しが要るなら止めてユーザーに聞く
- 完了の条件: 8章のすべてが期待どおり

## 7. 実行の順

```
T-0 → T-1 → T-2 → T-3 → T-4 → T-5 → T-6 → T-7 → T-8 → T-9
    → T-10 → T-11 → T-12 (W-D1〜W-D11) → T-13 → T-12 (W-D13〜W-D17)
    → T-14 → T-15 → T-16 → T-17 → T-18
```

- 1つずつコミットするので並べて流さない。T-4、T-5、T-6 は互いに入れ替えてよい。T-10 は T-1 の後ならどこでもよい
- T-13 は W-D11 (testing.md の数と列挙) の後、W-D13 の前に置く。付録 E の W-H13 は T-13 の後である
- 実行の最後に orchestrated-execution の手順で計画書 (`docs/plans/2026-10-21-docs-current-state/`) を消す。消すコミットで `citations.rs` の `docs/plans` の除外は残す (D-4 で今後の段も使う)

## 8. 全体の確かめ (T-18、計画書を消す前)

計画書のディレクトリは検索から外す。どれも 0fff343a では当たりがある。

```sh
# 段の名前: crates/eml_syntax/tests/corpus.rs の識別子 S1 と corpus/s1.em の中だけが残る
git ls-files | grep -vE '^docs/(reports|plans)/' | xargs grep -nE '\bS[0-9]+[a-z]?(-[0-9a-z]+)?\b'
# 日付: 0件
git ls-files '*.md' | grep -vE '^docs/(reports|plans)/' | xargs grep -nE '\b20[0-9]{2}-[0-9]{2}-[0-9]{2}\b'
# docs/plans と docs/superpowers への言及: citations.rs、docs/implementation/testing.md、docs/README.md、docs/future/roadmap.md、CLAUDE.md の決まりの文だけ
grep -rn 'docs/plans\|docs/superpowers' crates std tests bench docs CLAUDE.md README.md --exclude-dir=plans
# 記録の節と -dirty: どちらも 0
grep -c '^### S' docs/implementation/benchmarks.md
grep -c dirty docs/implementation/benchmarks.md
# docs/superpowers がない
ls docs/superpowers
# 基準のプログラムの表の行の数が bench/*.em の数と同じ
awk '/^## 基準のプログラム/{f=1;next} /^## /{f=0} f && /^\| `/' docs/implementation/benchmarks.md | wc -l
ls bench/*.em | wc -l
```

- `cargo test` が全部通る (`citations::` は T-1 が足したテストを含めて4件)
- `cargo clippy --all-targets -- -D warnings` と `cargo fmt --check` が通る
- `cargo verus verify -p eml_rt` と `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` が通る
- `cargo test -p eml_vm --test integration bench::` が通る

## 9. 未決の事項

| ID | 事項 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| U-1 | roadmap.md の出典のない7つの欄 (120 Kind の宣言化、126 row の引数の省略、134 多相なローカル関数、195 関数をまたぐ reuse、196 合流をまたぐ token の「見直す条件」と、154 `var` の残る制限、187 再帰する生産者の継続についての特殊化の「後に回した理由」) | T-9 | T-9 の前に実装者がユーザーに聞く | その欄を書かず、T-9 のコミットのメッセージに書かなかった欄を挙げる |
| U-2 | `codegen-units` の所見を測り直して、主張が成り立たなかったとき | T-17 | 実装者がユーザーに聞く | C1 に進まない |
| U-3 | 測る機種が Mac17,2 でないとき | T-17 | 付録 C の M の手順1 | 進め、C1 のメッセージに機種が変わったことを書く |
| U-4 | S14e の最終レビューの Minor 1 と Minor 6 | なし (範囲外) | ユーザーに伝える | 何もしない |

## 付録: レビューで変わった主な点

蒸し返さないための要点だけを書く。

- 決まりの文書化のコミットで、まだない見出しを CLAUDE.md が引くと検査が落ちる → 見出しを引く文はその見出しがあるコミットに入れる決まりにし、T-1 で benchmarks.md の「今の数値」と「所見」の節を先に作る
- 大きさの変更を閉じるコミットに入れると `-dirty` で測ることになる → 大きさの変更は別のコミットにする
- spec を実装より先に書くと、spec が規範でなくなる → spec は実装と同じコミットで書く
- 推測で「後に回した理由」を書くと、推測が正の文書の事実になる → 出典のある欄だけを書き、ほかはユーザーに聞く (U-1)
- 付録 C の初版の測り直しの awk は欄を取り違えていた → 付録 C の M の awk は行の名前で突き合わせる形に直してある
- 付録 D の初版の確かめの多くは、直す前と後で結果が同じだった → 付録 D、E の確かめは「前の値 → 後の値」の形にしてある
- コードのコメントの段の名前と、docs/ の経緯の記述は、初めどの作業にも割り当てがなかった → T-6 (コード) と T-14 (docs/) に割り当てた
