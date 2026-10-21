# 付録 A: 文書の決まり (S1)

計画書 plan.md が正である。この付録と plan.md が食い違うときは plan.md に従う。

## 1. 結論

### 問い1: 文書の役割

| 文書 | 書くこと | 書かないこと |
|---|---|---|
| `overview.md` | 目的、言語の性格、確定した設計判断の表 (spec へのリンク付き)、用語 | 段の一覧と経緯 (用語の「M1、M2」「S0〜S14」の行は消し、「段」の語の定義だけを残す)、「次は〜」 |
| `spec/` | 今の実装が満たす規則と、規則の理由 (なぜその形か)。読み手が最初に試しそうな別の形を採らなかった理由も、規則の隣に書く。未対応の構文は「`type` の宣言は HIR が E0004 を出す」のように、どの段階がどの診断を出すかを今の振る舞いとして書く | 段の名前、時期、前の状態との対比、計測の数値 (理由に要る数値は `benchmarks.md` の「所見」を引く)、将来の設計 (計画は `roadmap.md` の項目を引く) |
| `implementation/status.md` | `.em` を書く人の視点で見た今の言語の範囲 (実装したもの、まだないもの) と既知の制限。制限は定性的に書き、根拠の数値は `benchmarks.md` の「所見」を引く | 段の名前と経緯、処理系の内部構造 (今の「タプルの扱い」「HIR の位置」「verifier」の節は `architecture.md` か `spec/` へ)、`spec/` が定める規則の写し (1行とリンクにとどめる)、計測の数値 |
| `implementation/architecture.md` | crate の構成と規律、採らなかった形とその理由、処理系の内部の決め (HIR の位置の持ち方など) | 段の名前と経緯、段ごとの変更の記述 |
| `implementation/benchmarks.md` | 基準のプログラム、回数のテスト、測り方、今の数値の表 (3つ)、所見 (今も判断の根拠になる計測の要約。題名の見出し) | 段ごとの記録、段の前後の比較の表、段の名前の見出し |
| `future/roadmap.md` | 進め方、今後の項目 (前提、決めたこと、論点、順序の理由、後に回した理由と見直す条件) | 終えた段の節、「S13 では入れた」のような過去の記述、決定の日付、段の作業文書へのリンク |
| `future/` のほかの文書 | 今の実装を前提にした将来の設計 | 段の名前 (「S10 の VM は」→「VM は」)、未来形のまま残った過去 (「S14 で `Num` が入る」→ 今の状態に直す)、検討の日付 |
| `README.md` (docs) | 文書の地図、読む順、文書の扱い (下の文案) | 段の名前 (「次の段 (S13)」) |
| `reports/` | 書いた時点の調査。更新しない | 規則の出典にすること (規則が依るなら `spec/` に写す)。見出し付きの引用は `citations.rs` が断る |
| `docs/plans/` (新設) | 段の作業文書 (設計と計画)。段の最初のコミットで main に入れ、閉じるコミットで消す | 段を閉じた後に残ること。コードや文書から引くこと |

重なりの置き場所を決める基準は次のとおり。

- 制限の種類で置き場所を決める
  - 診断で検査する制限と、意味を定めた制限: `spec/` に書く。`status.md` は1行で挙げてリンクする
  - 未対応の構文 (E0004): `spec/` に「どの段階が E0004 を出すか」を今の振る舞いとして書く (`spec/diagnostics.md` の E0004 の項と、各構文の spec)。`status.md` の「まだないもの」は機能の名前を1行で挙げて `roadmap.md` の項目を引く。`status.md` の今の「未対応の構文と E0004」の節は `spec/diagnostics.md` へ移す
  - まだ作っていないことと遅いこと: `status.md` だけに書く
- 数値は `benchmarks.md` だけが持つ。`status.md` と `spec/` は「2乗の時間がかかる」「Python より遅い」のように定性的に書き、根拠が要る所で `benchmarks.md` の「所見」の見出しを引く。利用者が当たる上限 (`let` の列は 20000 まで、など) は数値のまま `status.md` に書いてよい。これは計測の数値ではなく制限の値である
- 速さの目標 (Python 3.14 の 1.5 倍以下) は `roadmap.md` (ネイティブ化の完了の条件) に書き、今の比は `benchmarks.md` の表だけが持つ

### 問い2: 経緯の記述の扱い

| 種類 | `spec/` | `implementation/` | `future/` |
|---|---|---|---|
| 1. 段の名前で時期を書く | 消して今の状態の言い方にする | 同じ | 同じ。`roadmap.md` の「前提: S14」は項目の名前か「なし」にする |
| 2. 前の状態との対比 | 今の状態の記述にする。対比が規則の理由なら「〜にすると〜になる (ため採らない)」の形で理由として残し、数値は「所見」へ | 同じ。`status.md` の evidence passing の費用は、1回の操作で何を実行するかの記述だけにし、段ごとの増減は消す | 同じ |
| 3. 採らなかった案と理由 | 規則の隣に残す (「採らなかった案」の節のまま) | `architecture.md` の「採らなかった形」に残す | 将来の項目の採らない案は `roadmap.md` の項目に残す |
| 4. 決定の記録 (誰がいつ決めたか) | 消す。決定の中身 (「後に回す」) は項目として残す | 同じ | 同じ。`roadmap.md` の後に回した項目は「後に回した理由: …。見直す条件: …」の形で書く。日付と決め手は閉じるコミットのメッセージに書く |
| 5. 未実装の予告 | 「今は〜」は今の規則を述べるときだけ使う。「後で入れる」は消し、要るなら `roadmap.md` の項目を引く | `status.md` の「まだないもの」が置き場所 | そのまま (将来の設計の文書なので) |
| 6. 未来形のまま残った過去 | 誤りとして今の状態に直す | 同じ | 同じ |
| 7. 記録の節への根拠の引用 | `benchmarks.md` の「所見」の題名の見出しに付け替える。所見に残さない数値なら、引用を外して理由を言葉で書く | 同じ | 同じ |

- 経緯そのものは git に任せる。段の名前、日付、前後の数値は、閉じるコミットのメッセージと `benchmarks.md` の差分 (`git log -p docs/implementation/benchmarks.md`) に残る。`benchmarks.md` の差分が比較にならない段 (機種か大きさが変わった段) は、閉じるコミットのメッセージにそう書く。書き忘れを捕まえる仕組みは置かない
- 日付を書くのは `reports/` だけにする。`future/verification.md` の「(2026-10-15)」も消す。コミットのハッシュを書くのは `benchmarks.md` (表の環境と所見) だけにする

### 問い3: `benchmarks.md` の形

「今の状態の説明」は次の6節で、「記録」の節は持たない。

1. 冒頭: 目的、2つの費用の見方、表を入れ替える決まり (文案は5章)
2. 基準のプログラム: `bench/*.em` の全部の表 (今は 36 本に対して表が 29 行 [V] `ls bench/*.em | wc -l` と表の行数)。「(S13e で足した)」のような由来は消す
3. 回数のテスト: `bench.rs` が固定するものと、組の比べのテストの説明。「S13e-1 の完了の条件」はテストの説明 (何と何が等しいことを確かめるか) に書き換える
4. 測り方: `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` と大きさの決め方
5. 今の数値: `run.sh`、`rt.sh`、`scripts.sh` の3つの表。各表の前に環境 (機種、OS、rustc、Python、コミット) を書く
6. 所見: 今もコードや文書の判断の根拠になっている計測の要約。見出しは題名 (「`eml_rt` のインライン化」「`INLINE_LIMIT` と `GROWTH_FACTOR`」「ネイティブ化の見込み」など) にし、各所見は次の4つを書く
   - 主張 (何がどうなるか)
   - 支える数値と、測った機種とコミット (短いハッシュ。日付は書かない)
   - 根拠にしている所 (この所見を引く規則、コメント、項目へのリンクか名指し)
   - 受け入れた後退なら、受け入れた理由

所見を残す基準は次のどれかに当たることである。

- コードか `docs/` の規則が今も引く
- `roadmap.md` の項目の根拠である
- 受け入れた後退 (今も残っている費用の増え) と、その理由である

所見の見直しは段を閉じるときに行う。段が変えた規則 (spec の節、しきい値、`#[inline]` の置き方など) を「根拠にしている所」に挙げる所見は、測り直して数値とコミットを更新するか、根拠でなくなったなら消す。段が触れていない所見は、そのまま残す。所見の数値は「今の数値」の表とは独立で、表の入れ替えで更新しない。

段の前後の比較は次の2つに残し、文書には残さない。

- `benchmarks.md` の差分。閉じるコミットが表を入れ替えるので、段ごとの前後は `git log -p` で読める
- 閉じるコミットのメッセージ。目立つ差 (何が何倍になったか) を数行で書く

表の入れ替えの決まりは次のとおり。

- 段を閉じるときに、3つの表を同じ機種で測り直して入れ替える。段の途中では入れ替えない (`status.md` と同じ扱い)
- 測り直しは前の表のコミットから `crates/eml_vm`、`crates/eml_rt`、`crates/eml_lir`、`crates/eml_core_ir`、`std/`、`bench/` のどれかが変わった段で行う (`git diff --stat <前の表のコミット> HEAD -- <6つのパス>` が空でない)。どれも変わっていない段は、表をそのまま残す
- 測るのは、コードの最後のコミット (大きさを変えたなら、その変更のコミット) の作業ツリーで、`-dirty` の値は載せない。表の「コミット」は閉じるコミットの1つ前になる
- 機種が前の表と違うときは、3つとも測り直し、閉じるコミットのメッセージに「機種が変わったので前の表と比べられない」と書く
- 測れる環境 (macOS) がないときは、閉じるコミットには表を入れず、表の入れ替えだけを直後の別のコミット (`Measure <段>`) にする。閉じるコミットのメッセージにそう書く

`scripts.sh` の大きさの決め直しは「前の段の記録と比べる」をやめて次の形にする。

- 各行の大きさは `bench/scripts.sh` が持つ固定の値で、記録する機種で eml の実時間がおよそ 300 ms になる値にする
- 段を閉じる計測で 150 ms 未満か 600 ms 超の行があれば、`bench/scripts.sh` の大きさを変えて、閉じるコミットの前の別のコミット (`Resize bench rows`) にする。その後に3つの表を測り直す。表に載せるのは新しい大きさの値だけで、前の大きさの値は git の履歴にある
- 大きさを変えた行と前後の値は、閉じるコミットのメッセージに書く。比べる相手は Python との比であり、小さな差は `instructions retired` で確かめる

### 問い4: 段のライフサイクル

- 段の作業文書 (設計と計画。1つのファイルでも2つでもよい) は `docs/plans/YYYY-MM-DD-<段>.md` に置く。`docs/superpowers/` は使わない。orchestrated-planning が `docs/plans/` に書き、orchestrated-execution が最後に消す [V] 両スキルの `SKILL.md` を grep
- 作業文書は、ユーザーが承認した版を段の最初のコミットで main に入れる。承認はチャットで取り、承認した版がコミットである。段の途中で作業文書を変えるときもコミットする。承認の範囲と日は `git log -- docs/plans/<ファイル>` で引ける
- `spec/` は段の中で書く。規則を実装するコミットと同じコミットで `spec/` の節を書き、コードのコメントはその節を引く。先に書かない。どのコミットでも、`spec/` が書く規則は、そのコミットのコードが満たす。段を途中でやめるときは、コードと `spec/` を一緒に戻す。コードと `docs/` から `docs/plans/` を引かない
- 段を閉じるときに、作業文書は消す。消す前に、`docs/` に残っていない決めがないことを確かめる (履歴は git に残る)
- 閉じるときに直す文書と順序は次のとおり。順序は引用の向き (後の文書が前の文書の見出しを引く) で決めている
  1. `spec/` の仕上げ (段の名前が残っていないこと)
  2. 計測: `bench/scripts.sh` を流し、範囲を外れた行があれば大きさを変えてコミットする。次に、測り直しの条件に当たれば、3つを流して `benchmarks.md` の「今の数値」を入れ替える。段が変えた規則を根拠にしている所見を更新するか消し、新しい所見を足す
  3. `status.md`
  4. `architecture.md` (crate かパスの構成が変わったとき)
  5. `roadmap.md` (段の節を消し、残した項目を「その後の項目」へ「後に回した理由」と「見直す条件」付きで移し、「前提」の段の名前を直す)
  6. `overview.md` の確定した設計判断の表 (判断が変わったとき) と `README.md` の地図の「内容」の欄
  7. `CLAUDE.md` (規則か道しるべが変わったときだけ)
  8. `docs/plans/` の作業文書を消す
  9. 1つのコミット「Close <段>」にまとめる (文案は5章)。大きさの変更と、測れない環境での表の入れ替えだけは別のコミットにする
- 閉じる単位は作業文書を持った単位 (S14c-3 のような小さい段も、作業文書があれば閉じる)。測り直しの条件に当たらない段は、計測を省く

### 問い5: 引用の方針

- 引く見出しは題名の見出し (規則、概念、計測の題) だけにする。段の名前、日付、記録の節の見出しは引かない。`docs/` の見出しに段の名前と日付を付けない (`reports/` は除く)
- 規則はその規則を定める `spec/` の節を引く。数値は `benchmarks.md` の「所見」を引く。制限は `status.md` を引いてよい (`scaling.rs` のように、制限を固定するテストから)。`reports/` の見出しは引かない
- 見出しなしの引用 (`(docs/spec/core-ir.md)`) は、文書全体を指すときだけにする
- 見出しを引く文は、その見出しが存在するコミットに入れる。見出しを消すか改名するときは、同じコミットで引く側を直す。`citations.rs` が `crates`、`docs`、`tests`、`std`、`CLAUDE.md`、`README.md` を検査する [V] `citations.rs` の `sources`。`bench/` は検査されないので、検査に加える (拡張子 `py`、`sh` と、コメント記号 `#` を足す)。`docs/reports` への見出し付きの引用を誤りにする検査も足す。同じファイルの中の「上の「…」」は検査されない [V] `citations.rs` のコメント。節を動かすときはそのファイルの中を grep する
- `docs/plans/` は検査から外す (`docs/superpowers` の除外を付け替える)。`docs/plans/` の見出しは外から引かない

### 問い6: 決まりを書く場所

| 決まり | 場所 |
|---|---|
| 各文書の役割、経緯の扱い、見出しと引用の方針 | `docs/README.md` の「文書の扱い」(書き直し) と地図の該当行 |
| 段の進め方と閉じる手順、後に回す項目の形 | `roadmap.md` の「進め方」(書き直し) |
| 作業文書の置き場所と、テストの変更の合意 | `testing.md` の「テストの変更の運用」(「作業の spec」→「段の作業文書」) |
| 引用の検査の範囲 | `testing.md` の「文書の引用の検査」 |
| 表の入れ替えと所見の決まり | `benchmarks.md` の冒頭 |
| 作業の規則と道しるべ | `CLAUDE.md` の Rules、Testing、Architecture の表 |

文案は5章にある。

## 2. 根拠

- 膨らみの原因は「段ごとに節を足す」という書き方そのものである。`benchmarks.md` の「記録」38節のうち、外から引かれる節は 20 で、6つ以上のファイルから引かれるのは S14e-1 と S14e-2 だけ [V] 見出しごとに `grep -rlF` で数えた (7章)。引かれる節も、引く側が使うのは数値でなく「こうすると遅くなるので、この形にした」という所見である (`crates/eml_rt/src/heap/mod.rs:149-151`、`alloc.rs:173-175` [V])。所見だけを題名の見出しで残せば、引用は保たれ、表は最新の1組で足りる
- 段の前後の比較を `git log -p` に任せられるのは、閉じるコミットが表をそのまま置き換えるからである。差分が比較になる。今の「記録」の比較の表 (S9b-2a の「S9b-1 との差」など) は、同じ情報を手で写している
- `spec/` を段の中で書く形にすると、閉じるコミットから「コードのコメントの引用を付け替える」仕事が消える。今は閉じるコミットがそれを担っている [V] コミット bfc8f986 のメッセージ「Repoint every code comment that cited the S14e design spec」。また `README.md` の「仕様を変えるときは、まず `spec/` の該当文書を直す」とも合う。実装と同じコミットに限るのは、どのコミットでも `spec/` が今の実装の規範であり続けるためで、テストの変更の合意は作業文書だけが持つ
- 作業文書を段の最初のコミットで main に入れるのは、orchestrated-execution が計画書をブランチの最初のコミットにする手順と同じで [V] `SKILL.md:57`、承認の記録を git に残すためである
- 段の作業文書を消す前提 (決定済み) の下では、作業文書を引く引用は必ず壊れる。今も `tests/ui/run/effects/` の3本の `.em` が `docs/superpowers` を引いている [V] `grep -rln 'docs/superpowers' tests`。見出しなしの引用なので `citations.rs` は捕まえない。「`docs/plans/` を引かない」と決め、閉じる手順の grep で確かめるのが、検査なしで守れる最も簡単な形である
- 決定の日付を消してよいのは、日付が「何を作るか」を変えないからである。後に回した項目で要るのは日付でなく「見直す条件」で、それは項目に書く。日付は `git log -S` で引ける
- 文書を直す順序を引用の向きで決めるのは、`citations.rs` が見出しの完全一致を見るからである。先に `benchmarks.md` の所見の見出しを決めないと、`status.md` と `spec/` の引用先が書けない
- `status.md` から処理系の内部の節を外すのは、`README.md` の地図が `status.md` を「今の言語の範囲、既知の制限」と定めているからで、「タプルの扱い」(HIR の表現) と「verifier」はこれに当たらない
- 大きさの変更を別のコミットにするのは、「`-dirty` を載せない」と「大きさを変えて測り直す」を両立させるためである。`bench/scripts.sh` はコードなので、変えた木で測るとコミットのハッシュがない
- 測り直しの条件を6つのパスで決めるのは、命令の数が変わりうる変更がそこにしかないからである。`RunStats` のスナップショットが変わらなくても VM の実装の変更で命令の数は変わるので、スナップショットの差では決めない

## 3. 検討して退けた案

- 「記録」を別のファイル (`docs/implementation/benchmarks-history.md`) に移す: 膨らみが場所を変えるだけで、引用の付け替えも同じだけ要る。読み返される証拠もない
- 段の前後の比較を各段の `spec/` の節に残す: `spec/` に段の名前と時期が戻り、問い2の決まりと矛盾する
- `docs/superpowers/` を置き場所のまま残す: `citations.rs` の除外と skill の既定 (`docs/plans/`) の2つを合わせるために、1つに寄せた
- 作業文書を消さずに `docs/plans/done/` に移す: 承認の記録は作業文書を main にコミットすることで git に残る (`git log -- docs/plans/<ファイル>`)。`done/` は同じ記録の写しになり、段の名前と日付を持つ文書が `docs/` に増え続ける
- 閉じるときに `spec/` をまとめて書く (今のやり方): 閉じるコミットが大きくなり、段の途中でコードのコメントが作業文書を引く。付け替えの仕事が毎回出る
- `spec/` を実装より先に書く: コミットの時点で `spec/` が未実装の規則を持ち、規範でなくなる。段を途中でやめたときに戻し忘れる
- `rt.sh` は `eml_rt` に触れた段だけ測り直す: 表の機種と日付がばらばらになり、3つの表を同じ環境で読めなくなる。代わりに、6つのパスのどれにも触れていない段は3つとも省く
- 決定の日付を `roadmap.md` に残す: 日付は項目の中身を変えない。残すと「決定の記録」の種類が文書に戻る。見直す条件を書くほうが長く使える
- 段の名前の見出しを引くことを許し、改名時に `citations.rs` で直す: 段の名前の見出しは閉じるたびに増え、消すときに引用がまとめて壊れる。今回の S3 がまさにその仕事である
- 所見に日付を添える: 日付の禁止と食い違う。コミットのハッシュは `git show` で日付と中身に戻れる

## 4. 置いた仮定

- 段の前後の数値を読み返す場面は、`git log -p docs/implementation/benchmarks.md` で足りる [I]。外れたら (段ごとの表を1か所で見たい場面が出たら)、`git log` の出力を集める小さなスクリプトで補える。文書の決まりは変えない
- `docs/plans/` の文書は orchestrated-planning / orchestrated-execution 以外の流れ (superpowers の brainstorming と writing-plans) でも同じ場所に書ける [I]。外れたら、skill の側の出力先を合わせる
- `reports/` は今回の整理の対象にしない [I]。`README.md` の「後から更新しない」の定めに従う。対象にするなら、同じ基準 (日付と段の名前を持つ唯一の場所) だけを書き足す
- 閉じるコミットのメッセージに「機種か大きさが変わった」と書き忘れる危険は受け入れる [I]。表の環境の欄 (機種、大きさ) が差分に出るので、読み手は気付ける

## 5. 実装への含意

### 守る制約

- `docs/` の文書は今の状態を書く。段の名前、日付、前の状態との対比、決定の記録は書かない。例外は `reports/`。コミットのハッシュは `benchmarks.md` だけに書く
- `docs/` の見出しに段の名前と日付を付けない。コードと文書が引くのは題名の見出しだけである
- 数値は `benchmarks.md` だけが持つ。ほかの文書は定性的に書き、「所見」を引く
- コードと `docs/` から `docs/plans/` と `docs/reports/` の見出しを引かない
- 見出しを引く文は、その見出しが存在するコミットに入れる。見出しを消すか改名するコミットは、引く側を同じコミットで直す
- `spec/` の節は規則を実装するコミットと同じコミットで書く
- `benchmarks.md` の表は段を閉じるときだけ入れ替え、`-dirty` の値を載せない
- 今回の整理の途中 (単位1の後、S4 の前) は、`docs/` が決まりに違反した状態で main にある。移行中であることは計画書 (`docs/plans/2026-10-21-<topic>.md`) とコミットメッセージにだけ書き、`docs/` には書かない

### 作業の単位 (S2〜S4 と、その前に済ませる単位)

1. 決まりの文書化と検査の整え。1つのコミットで次を行う
   - `citations.rs`: 除外を `docs/superpowers` から `docs/plans` に付け替える。`bench/` を走査に加える (拡張子 `py`、`sh`、コメント記号 `#`)。`docs/reports` への見出し付きの引用を誤りにする
   - `benchmarks.md`: 冒頭を文案に置き換え、「今の数値」の節を作って、今ある最新の表 (`run.sh` と `scripts.sh` は「S14e-2」、`rt.sh` は「S13e-4」の表を環境ごと写す) を置き、空の「所見」の節を作る。「記録」の節は S3 が消すまで残す。この時点では「今の数値」と「記録」に同じ表が重なる
   - `CLAUDE.md`、`docs/README.md`、`roadmap.md` の「進め方」、`testing.md` の2か所を文案に置き換える
   - 完了の条件: `cargo test -p eml_cli --test integration citations::` が通る (「今の数値」「所見」「文書の引用の検査」「進め方」「文書の扱い」の見出しが、この時点で存在する)
2. S2: `status.md` と `roadmap.md` の書き直し (章立てと項目ごとの行き先)。S3 の所見の見出しを引くので、S3 の後か、S3 と見出しの名前を合わせて同時に進める
3. S3: `benchmarks.md` の書き直し (所見の選別と見出し、測り直し、「記録」の削除、引用の付け替えの対応表)。所見の見出しは `status.md` と `spec/` が引くので、S2 と S4 より先に決める
4. S4: `spec/` の食い違いの直しと、経緯の記述の除去 (`spec/`、`architecture.md`、`testing.md`、`future/`、`overview.md`)。`docs/superpowers/` の削除と、`tests/ui/run/effects/` の3本の `.em` の引用の付け替えを含む

### 完了の条件 (S4 と計画書の削除の後に確かめる)

- `git ls-files | grep -v '^docs/reports/' | xargs grep -nE '\bS[0-9]+[a-z]?(-[0-9a-z]+)?\b'` の一致が `crates/eml_syntax/tests/corpus.rs` の識別子 `S1` と `crates/eml_syntax/tests/corpus/s1.em` の中だけ (コーパスの名前。段の名前ではない [V] `const S1: &str = include_str!("corpus/s1.em")`。`LATER_STAGES` は正規表現に当たらない)。今の時点で `.rs`、`.em`、`.md`、`.py`、`.sh` 以外のファイル (`.toml`、`.nix`、`.snap` など) に一致はなく、段の名前に由来するファイル名とディレクトリ名も `corpus/s1.em` だけである [V] `git ls-files | grep -vE '\.(rs|em|md|py|sh)$' | xargs grep -nE …` と `git ls-files | grep -iE '(^|/)s[0-9]+[a-z]?(-|_|\.|/)'`
- `git ls-files | grep -v '^docs/reports/' | xargs grep -nE '\b20[0-9]{2}-[0-9]{2}-[0-9]{2}\b' -- '*.md'` が0件
- `grep -rn 'docs/plans\|docs/superpowers' crates std tests bench docs CLAUDE.md README.md` の一致が `crates/eml_cli/tests/citations.rs`、`docs/implementation/testing.md`、`docs/README.md`、`docs/future/roadmap.md`、`CLAUDE.md` の決まりの文だけ
- `grep -c '^### S' docs/implementation/benchmarks.md` が 0、`grep -c dirty docs/implementation/benchmarks.md` が 0
- `ls docs/superpowers docs/plans` が、どちらも「No such file or directory」
- `cargo test -p eml_cli --test integration citations::` が通る (`every_cited_heading_exists`、`every_linked_document_exists`、`unclosed_code_is_an_error` と、`reports/` への引用を断る新しいテスト)
- `cargo test -p eml_vm --test integration bench::every_program_in_bench_has_a_test` が通り、`benchmarks.md` の基準のプログラムの表の行数が `ls bench/*.em | wc -l` と等しい

### 文案

#### `CLAUDE.md` (英語。変える行だけ)

Rules に次の2行を足す。

```
- The documents under `docs/` describe the current state. They do not name stages, dates, or what came before; that history lives in git (docs/README.md の「文書の扱い」). The only exception is `docs/reports/`.
- A stage's design and plan live in `docs/plans/`, are committed on main as the stage's first commit, and are deleted when the stage closes (docs/future/roadmap.md の「進め方」). Code and `docs/` never cite `docs/plans/`; write the `docs/spec/` section in the same commit that implements the rule, and cite that.
```

Rules の "When a test fails and there is a sound reason that the test itself is what should change, put the change in the work's spec with its kind (see Testing); approving the spec is the agreement." を次に置き換える。

```
- When a test fails and there is a sound reason that the test itself is what should change, put the change in the stage's plan under `docs/plans/` with its kind (see Testing); approving the plan is the agreement.
```

Testing の "The `bench/` programs have their `RunStats` pinned ... (docs/implementation/benchmarks.md の「記録」)." を次に置き換える。

```
- The `bench/` programs have their `RunStats` pinned by inline snapshots in `crates/eml_vm/tests/bench.rs`, so a change in work counts is an expected-value change. When a stage that touched `eml_vm`, `eml_rt`, `eml_lir`, `eml_core_ir`, `std/` or `bench/` closes, replace the three tables in docs/implementation/benchmarks.md の「今の数値」 with fresh `bench/run.sh`, `bench/rt.sh` and `bench/scripts.sh` output measured on a clean tree; the stage-by-stage history is the file's git log.
```

Testing の "Test changes come in three kinds ... are listed one by one in the work's spec; ... described as a scope in the work's spec ...; approving the spec is the agreement for kinds 1 and 2, and the reason goes in the spec and the commit message; ..." の中の "the work's spec" を "the stage's plan" に、"approving the spec" を "approving the plan" に、"the reason goes in the spec and the commit message" を "the reason goes in the plan and the commit message" に置き換える。次の行 "Never bend the design ... Put the test change in the work's spec instead" も "in the stage's plan" にする。

Architecture の表の行 "| The implemented language and its limits | docs/implementation/status.md; the order of the work is in docs/future/roadmap.md |" を次に置き換える。

```
| The implemented language and its limits | docs/implementation/status.md; what comes next and why is in docs/future/roadmap.md |
```

#### `docs/README.md` の地図の行

```
| [implementation/status.md](implementation/status.md) | 手引き | 今の言語の範囲 (実装したもの、まだないもの)、既知の制限 |
| [implementation/benchmarks.md](implementation/benchmarks.md) | 手引き | 基準のプログラム、回数のテスト、測り方、今の数値、所見 |
| [future/roadmap.md](future/roadmap.md) | 将来の設計 | 段の進め方と閉じる手順、今後の言語の項目と処理系の項目 |
| **plans/** | 作業中 | 段の設計と計画。段の最初のコミットで入れ、段を閉じるときに消す。コードと文書から引かない |
```

#### `docs/README.md` の「文書の扱い」(全文を置き換える)

```
## 文書の扱い

- `spec/` が実装の規範である。仕様を変えるときは、`spec/` の該当文書を、規則を実装するコミットと同じコミットで直す。どのコミットでも、`spec/` が書く規則はそのコミットのコードが満たす。コードのコメントは `spec/` の節を引く。
- `docs/` の文書は、今の状態を書く。段の名前と時期、前の状態との対比、決定の日付は書かない。経緯は git の履歴に任せる。例外は `reports/` で、書いた時点のまま残す。コミットのハッシュを書くのは `implementation/benchmarks.md` だけである。
- 規則の理由と、採らなかった案は、その規則を定める文書の隣に書く。時期は書かず、「〜にすると〜になるため採らない」のように今の形の理由として書く。
- 制限の置き場所は、診断で検査するか意味を定めた制限なら `spec/`、まだ作っていないことと遅いことなら `implementation/status.md` である。未対応の構文は、どの段階が E0004 を出すかを `spec/` に書き、`status.md` の「まだないもの」は機能を1行で挙げて `future/roadmap.md` の項目を引く。`status.md` は `spec/` の制限を1行で挙げてリンクする。
- 計測の数値は `implementation/benchmarks.md` だけが持つ。ほかの文書は定性的に書き、根拠が要る所で `benchmarks.md` の「所見」を引く。
- `implementation/status.md` と `implementation/architecture.md` と `benchmarks.md` の表は、段の途中では直さず、段を閉じるときに直す ([ロードマップ](future/roadmap.md) の「進め方」)。
- `future/` の内容はまだ実装しない。実装に進むときは、決まった部分を `spec/` に移す。今後の項目の一覧は [future/roadmap.md](future/roadmap.md) を正とする。マルチコアと標準ライブラリの詳細は、それぞれ [future/multicore.md](future/multicore.md) と [future/stdlib.md](future/stdlib.md) にある。
- 段の設計と計画は `plans/` に置き、段の最初のコミットで入れ、段を閉じるときに消す。コードと `docs/` から `plans/` と `reports/` の見出しを引かない。
- 見出しは題名 (規則、概念、計測の題) にし、段の名前と日付を付けない。コードと文書が引くのは題名の見出しで、見出しを引く文はその見出しがあるコミットに入れ、見出しを消すか改名するときは同じコミットで引く側を直す ([テスト戦略](implementation/testing.md) の「文書の引用の検査」)。
- 構文で迷ったときは Haskell の慣習に寄せる。
- 文書は日本語で書く。
```

#### `docs/future/roadmap.md` の「進め方」(全文を置き換える)

```
## 進め方

- 各段は、設計、計画、実装、閉じる作業の順に進める。設計は、この文書のその段の項目を出発点にし、設計と計画は `docs/plans/YYYY-MM-DD-<段>.md` に書く。承認した版を段の最初のコミットで main に入れ、途中で変えたときもコミットする
- 「決めたこと」は方針で、細部はその段の設計で決める。「論点」は、その段の設計で決める
- `spec/` は段の中で書く。規則を実装するコミットと同じコミットで `spec/` の節を書き、コードのコメントはその節を引く。設計の文書は引かない。段を途中でやめるときは、コードと `spec/` を一緒に戻す
- テストの変更は、段の設計に、[テスト戦略](../implementation/testing.md) の「テストの変更の運用」の種類 (成否の変更、期待値の変更、機械的な追随) で書く
- 段で後に回した項目は、この文書の「その後の項目」に「後に回した理由: …。見直す条件: …」の形で書く。決めた日と決め手は書かず、閉じるコミットのメッセージに残す
- 段を閉じるときは、次の順に文書を直し、1つのコミット (`Close <段>`) にまとめる。順序は引用の向きで決めてある
  1. `spec/` を仕上げる。段の名前と「今は〜、後で〜」の予告が残っていないことを確かめる
  2. 計測する。前の表のコミットから `crates/eml_vm`、`crates/eml_rt`、`crates/eml_lir`、`crates/eml_core_ir`、`std/`、`bench/` のどれかが変わった段だけが対象である。まず `bench/scripts.sh` を流し、実時間が範囲を外れた行があれば `bench/scripts.sh` の大きさを変えて別のコミット (`Resize bench rows`) にする。次に `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` をコードの最後のコミットの作業ツリーで流し、[計測の基準](../implementation/benchmarks.md) の「今の数値」の表を入れ替える。段が変えた規則を根拠にしている「所見」は測り直して更新するか消し、今後も判断の根拠になる計測は題名の見出しで足す。測れる環境がないときは、表の入れ替えだけを直後の別のコミット (`Measure <段>`) にする
  3. [実装の現在地](../implementation/status.md) の範囲と制限を直す
  4. [コンパイラの構成](../implementation/architecture.md) を、crate かパスの構成が変わったときに直す
  5. この文書からその段の節を消す。残した項目は「後に回した理由」と「見直す条件」を添えて「その後の項目」へ移し、「前提」に書いた段の名前を項目の名前に直す
  6. [概要](../overview.md) の確定した設計判断の表と、[文書の地図](../README.md) の「内容」の欄を、変わった所だけ直す
  7. `CLAUDE.md` を、規則か道しるべが変わったときだけ直す
  8. `docs/plans/` の設計と計画を消す。消す前に、`docs/` に残っていない決めがないことを確かめる
  9. コミットのメッセージに、文書で何をどこへ移したか、テストの変更の種類と理由、計測の目立つ差、大きさを変えた行、機種が変わったこと、後に回した項目の決めた日を書く。段の名前と日付はここに残る
- 段は、パイプラインの段階 (crate) とは別の呼び方である
```

#### 閉じるコミットのメッセージの形

```
Close <段>

<段が入れたものを1〜2文>。spec: <直した文書と節>。status/roadmap: <消した節、移した項目>。
benchmarks: <入れ替えた表と目立つ差。大きさを変えた行と前後の値。機種が変わったなら、そのこと。測らなかったなら、その条件>。

Tests: <成否の変更を1件ずつ>。<期待値の変更の範囲と理由>。
Decided: <後に回した項目と理由、決めた日>。

Removed docs/plans/<ファイル> (approved <日付>).
```

#### `docs/implementation/testing.md`

「テストの変更の運用」の表の「合意と記録」の欄と、下の箇条の「作業の spec」を「段の設計 (`docs/plans/`)」に置き換える。表の1行目は「段の設計に1件ずつ挙げる。設計の承認を合意とみなす」、2行目は「段の設計に、変わるテストを範囲で書く (…)。設計の承認を、その範囲の変更すべてへの合意とみなす」になる。箇条の「作業の計画の全体制約は『成否と期待値は、spec に挙げたテストと範囲の中だけで変える。期待値を変えない機械的な追随は許す』と書く」は「設計に挙げたテストと範囲の中だけで変える」にする。箇条に「承認した設計は段の最初のコミットで main に入れるので、承認の範囲と日は `git log -- docs/plans/<ファイル>` で引ける」を足す。

「文書の引用の検査」の最初の段落と最後の段落を次に置き換える。

```
`crates/eml_cli/tests/citations.rs` は、コメントと文書の引用を3つ確かめる。走査するのは `crates`、`docs`、`tests`、`std`、`bench` と、根の `CLAUDE.md` と `README.md` である。標準ライブラリの `.em` のコメントと、`bench/` の `.em`、`.py`、`.sh` のコメントも、`docs/…` の見出しを引ける。
```

箇条に次を足す。

```
- `docs/reports/` の見出しを引いていない。調査レポートは書いた時点のまま残すので、規則の出典にしない ([文書の地図](../README.md) の「文書の扱い」)
```

```
見出しのない引用 (`(docs/spec/core-ir.md)` など) は検査できないので、文書全体を指すときだけに使う。同じファイルの中の「上の「…」」も検査しないので、節を動かすときはそのファイルの中を grep する。`docs/plans` は段の設計と計画で、段を閉じるときに消すので、検査から外し、コードと文書からも引かない ([ロードマップ](../future/roadmap.md) の「進め方」)。引く見出しは題名の見出しに限り、段の名前の見出しは作らない。見出しを引く文は、その見出しがあるコミットに入れる ([文書の地図](../README.md) の「文書の扱い」)。
```

#### `docs/implementation/benchmarks.md` の冒頭 (「基準のプログラム」の前を置き換える)

```
# 計測の基準

位置づけ: 手引き。

実行の費用を測るための、基準のプログラムと測り方と、今の数値をまとめる。費用は2つの数で見る。

- 実行の仕事の回数 (`RunStats`): 決定的なので、テストのスナップショットで固定する。回数が変わると、スナップショットの差分として出る
- 実行した命令の数: macOS の `/usr/bin/time -l` が出す `instructions retired` を、`bench/run.sh` と `bench/rt.sh` で測る。スクリプトと数値計算は、Python と実時間で比べる (`bench/scripts.sh`)

下の「今の数値」の表は、段を閉じるときに、3つとも同じ機種で測り直して入れ替える ([ロードマップ](../future/roadmap.md) の「進め方」)。測り直すのは、前の表のコミットから `crates/eml_vm`、`crates/eml_rt`、`crates/eml_lir`、`crates/eml_core_ir`、`std/`、`bench/` のどれかが変わった段である。測るのはコードの最後のコミットの作業ツリーで、`-dirty` の値は載せない。段の途中では入れ替えない。段ごとの前後は、この文書の git の履歴 (`git log -p docs/implementation/benchmarks.md`) と、閉じるコミットのメッセージで読む。

今も判断の根拠になっている計測は、下の「所見」に題名の見出しで残し、コードと文書はその見出しを引く。各所見は、主張、支える数値と測った機種とコミット、根拠にしている所 (この所見を引く規則、コメント、項目)、受け入れた後退ならその理由を書く。残すのは、コードか `docs/` の規則が今も引くもの、[ロードマップ](../future/roadmap.md) の項目の根拠であるもの、受け入れた後退とその理由である。所見の数値は表と独立で、表の入れ替えでは更新しない。段が変えた規則を根拠にしている所見は、その段を閉じるときに測り直して更新するか、根拠でなくなったなら消す。
```

「スクリプトの計測」の「大きさ」の節を次に置き換える。

```
### 大きさ

行ごとの大きさ (入力の KiB か、プログラムに渡す大きさ) は `bench/scripts.sh` が持ち、次の規則で決める。

- 記録する機種で、eml の実時間 (3回の中央値) がおよそ 300 ms になる大きさにする。Python の実時間はその大きさでの値をそのまま記録する
- 段を閉じる計測で、eml の実時間が 150 ms より短いか 600 ms より長い行は、`bench/scripts.sh` の大きさを変えて、閉じるコミットの前の別のコミットにし、その後で3つの表を測り直す。表に載せるのは新しい大きさの値だけで、変えた行と前後の値は閉じるコミットのメッセージに書く
- 表の「大きさ」の欄に、どの大きさで測ったかを残す
- 実時間は 0.01 秒の単位で出るので、300 ms では約 3% の刻みになる。小さな差は `instructions retired` で確かめる。Python の実時間が数十 ms の行 (`wc`) では、比が刻みの分だけ大きく揺れる
- fannkuch-redux の大きさは順列の長さ n で、n が1つ増えると実時間が約9倍になる。300 ms に近い n がないので、300 ms との比が最も小さい n にする
```

## 6. 未決の事項

- 所見の見出しの名前の付け方 (題名の粒度) は S3 が決める。既定: 1つの規則か1つのしきい値につき1つの見出しにし、引く側が多い順に作る
- `status.md` から処理系の内部の節 (「タプルの扱い」「HIR の位置」「verifier」) をどこへ移すか。既定: 「タプルの扱い」と「HIR の位置」は `architecture.md` の `eml_hir` の節、「verifier」は `spec/lir.md` の verifier の節。「未対応の構文と E0004」は `spec/diagnostics.md` の E0004 の項。S2 と S4 が分担する
- `rt.sh` の表の26計測と `run.sh` の 36 本をすべて載せるか。既定: すべて載せる。減らすなら `bench/` から消す
- 段の設計と計画を1つのファイルにするか2つにするか。既定: skill の出力に合わせて1つ。2つにするなら `-design.md` と `-plan.md` の接尾辞にする
- 測り直しの条件の6つのパスで足りるか (たとえば `crates/eml_types` の変更は命令の数を変えない前提)。既定: 6つにし、外れたら足す

## 7. 追加の確認

- `benchmarks.md` の「記録」38節のうち、`benchmarks.md` 自身と `docs/superpowers` を除いた場所 (`crates`、`docs/spec`、`docs/implementation`、`docs/future`、`tests`、`std`、`bench`、`CLAUDE.md`) から引かれる節は 20 である。引かれない 18 節: S4a、S4b、S5、S6a、S6b、S6c、S8d、S9v、S11c-3、S12a、S12b、S12c、S13b-1、S13b、S13c、S14a、S14b、S14c-1 (`grep -rlF 'の「<見出し>」'` を見出しごとに数えた)。依頼の「15節」は `benchmarks.md` の中からの引用と `docs/superpowers` からの引用を数えに入れた値と見ている [I]
- `bench/` の `.em`、`.py`、`.sh` の見出し付きの引用 31 か所は、どれも行き先に見出しがある [V] `citations.rs` と同じ照合 (空白を除き、`#` と `--` のコメント記号を外す) を Python で書いて流した。壊れているものは 0 件。単位1で `bench/` を検査に加えても落ちない
- `docs/superpowers` を引くファイル: `citations.rs`、`tests/ui/run/effects/` の3本、`benchmarks.md`、`testing.md`、`roadmap.md` (`grep -rln`)。`crates` の `src/` にはない
- `docs/` の日付は `status.md:97`、`verification.md:5`、`README.md:42` (reports の行)、`roadmap.md:5,143,187` にある (`grep -rnoE '20[0-9]{2}-[0-9]{2}-[0-9]{2}'`)
- `docs/spec` と `implementation`、`future`、`overview`、`README` の段の名前の行数: `object-model.md` 18、`core-ir.md` 10、`vm.md` 7、`multicore.md` 24、`verification.md` 6、`stdlib.md` (future) 6、`testing.md` 4、ほかは 0〜2 (`grep -cE`)。`docs/` の見出しで段の名前を持つのは `benchmarks.md` の「記録」の節だけ (`grep -nE '^#+ .*\bS[0-9]'`)
- 段の名前は `.rs`、`.em`、`.md`、`.py`、`.sh` 以外の追跡ファイルにはなく (`git ls-files | grep -vE '\.(rs|em|md|py|sh)$' | xargs grep -nE '\bS[0-9]+…'` が0件)、`docs/` の外の `.md` (`crates/eml_rt/CLAUDE.md` など) にもない。段の名前に由来するファイル名は `crates/eml_syntax/tests/corpus/s1.em` だけ
- orchestrated-execution の Phase 3 は計画書をブランチの最初のコミットにし、Phase 6 は計画書を消すコミットを作り、「残す内容があれば、消す前に置き場所をユーザーに聞く」と書いてある (`~/.claude/skills/orchestrated-execution/SKILL.md:57,137`)。main に直接コミットする運用でも同じ形で使える

---

