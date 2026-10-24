# hash-choice

- 作成日: 2026-10-24
- 基点: claude/loving-ride-tjwfhl @ fx-hash を閉じた後
- 状態: 計画は確定。aarch64 の macOS で測った結果で、6章の決め方に従って決める

この計画は、コンパイラの段階のハッシュ表 (`FxHashMap`、`FxHashSet`) のハッシュ関数を、FxHash から aHash、foldhash、rapidhash のどれかに替えるべきかを決める段 `hash-choice` の設計と計画である。整理の段なので roadmap の「段の列」に載せない。付録 (測るスクリプトと代わりの crate) は同じ名前のディレクトリに置く。段を閉じるときに、この文書とディレクトリを消す。

## 1. 目的

- 対象の環境 (x86_64 の Linux と Windows、aarch64 の macOS) で、`eml check` と `eml run` の時間が最も短いハッシュ関数を選ぶ
- 選ぶのは表のハッシュ関数だけで、どの表を使うか (`HashMap` か `Vec` か) は変えない

## 2. 表の鍵

- 大半は処理系の中の ID (`u32` の newtype、`la-arena` の `Idx`) と、その組 (`(Ty, Ty)`、`(ExprId, u32)` など) である。型の表 (`TypeStore`) は、型の形 (`TypeKind`。ID の列を持つ enum) を1回ハッシュする
- 文字列の鍵は、関数の名前 (VM の `Links`、`eml_core_ir` の名前の表)、型変数の名前、REPL の表示済みの名前で、どれも短い
- 入力は利用者自身のプログラムなので、衝突を狙った入力への耐性は要らない。どの関数も種を固定し、実行ごとに順が変わらないようにして比べる

## 3. 候補と、x86_64 で分かったこと

| 関数 | 1語の混ぜ方 | x86_64 (既定の target) | aarch64 の macOS (既定の target) |
|---|---|---|---|
| FxHash (`rustc-hash` 2.1) | 加算と 64 ビットの乗算。`finish` で回転 | 乗算1つ | 乗算1つ (`mul`) |
| aHash 0.8 | AES の命令か、128 ビットの乗算を畳む予備の道 | AES の道は `-C target-feature=+aes` で組んだときだけ。既定の組み立て (`nix build` を含む) は予備の道 | AES の道は nightly の feature `nightly-arm-aes` が要る。stable では予備の道 |
| foldhash 0.2 (`fast`) | 128 ビットの乗算を畳む | `mul` 1つで上下の語 | `mul` と `umulh` |
| rapidhash 4.5 (`fast`) | 128 ビットの乗算を畳む | `mul` 1つで上下の語 | `mul` と `umulh` |

x86_64 の Linux (この計画を書いた環境) で、release の `eml` を4つ組んで比べた結果は次のとおりである。

- 命令の数 (callgrind、`bench/` の6本): FxHash が最も少ない。ほかはどれも FxHash より 0.1〜1.3% 多い。`+aes` で組んだ aHash も 0.1〜1.0% 多い
- CPU 時間 (41 回を交互に走らせた中央値、`nbody`、`std_map`、`spectral_norm`): どの候補も、FxHash との差は ±2% の揺れの中に収まった。`eml run spectral_norm` で aHash と foldhash が 8% 短かったが、差は VM の実行の部分にあり、表を引く回数はほかのプログラムと変わらないので、コードの配置の揺れとみなす
- 4つの `eml` の出力は、どのプログラムでも同じだった (出力が表の順に依存しないことの確かめ)

## 4. aarch64 の macOS での見込み

- aarch64 では、64 ビットの乗算 (`mul`) と上の語の乗算 (`umulh`) はどちらも速い。128 ビットの乗算を畳む foldhash と rapidhash は、x86_64 より FxHash との差が縮む見込みである
- 鍵の大半が1語か2語の ID なので、1語あたりの命令が最も少ない FxHash が不利になるのは、FxHash の質の低さで衝突が増える場合だけである。x86_64 では衝突の増加は命令の数に現れなかった
- aHash の AES の道は stable の Rust では aarch64 で使えないので、aarch64 での aHash は予備の道 (foldhash と同じ系統) になる

## 5. 測り方 (aarch64 の macOS で行う)

1. このブランチを checkout し、devShell に入る (`direnv` か `nix develop`)。devShell の外でも、`rust-toolchain.toml` の toolchain があれば動く
2. 次を流す。release の `eml` を4回組み立てるので、初回は数分かかる。代わりの crate の依存 (aHash、foldhash、rapidhash) を取るので、ネットワークが要る

   ```sh
   docs/plans/2026-10-24-hash-choice/hashers.sh 11 > hashers-$(uname -m).md
   ```

   引数は回数 (既定 11) と、`bench/` のプログラムの名前 (既定は10本) である。リポジトリの作業ツリーは変えず、一時のディレクトリに worktree を作って消す
3. 出力の Markdown (機種、OS、rustc、コミットと表) を、この計画のコミットか PR のコメントで共有する

スクリプトは `/usr/bin/time -l` の instructions retired と user + sys の時間を読む。電源につないだ状態で、ほかの重い処理を止めて流す。

## 6. 決め方

- 候補が、aarch64 の macOS で `eml check` の CPU 時間の中央値を、測ったプログラムの過半で 2% 以上縮め、どのプログラムでも 1% より長くせず、x86_64 (3章) でも 1% より長くしないときだけ、その候補に替える
- 当てはまる候補がなければ FxHash のままにする
- どちらの場合も、[計測の基準](../implementation/benchmarks.md) の「所見」に、測った機種と結果を題名の見出しで足し、`architecture.md` の「外部 crate」の表の理由をそれに合わせる

## 7. テストの変更

- 全体の制約: 成否と期待値は変えない。ハッシュ関数を替えるときも、期待値を変えない機械的な追随だけを許す

## 8. 閉じる作業

- 替えるときは、workspace の依存と対象の5つの crate の型の名前を替え、全体の検証のコマンドを流す
- この文書と `docs/plans/2026-10-24-hash-choice/` を消し、決めた理由と測った数値を閉じるコミットのメッセージに書く
