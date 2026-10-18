# S13e-4 reuse の設計

## 目的と範囲

S13e-4 (Perceus の reuse analysis) の設計である。S13e の設計 (`docs/superpowers/specs/2026-10-18-s13e-specialization-design.md`) の「S13e-4 reuse analysis」の節を、2026-10-19 の計測と議論で広げた。この文書は、その節と、同じ文書の「作業の分け方と完了の条件」の S13e-4 の行に代わる。S13e の設計のほかの節 (計測の記録、文書の更新、テストの変更の種類の考え方) は、そのまま S13e-4 にも当てはまる。

reuse は、関数の中で一意な箱を手放し、同じ物理的な語の数の箱を作るときに、解放と確保をやめて箱を使い回す最適化である。同じコンストラクタで作り直すときは、変わらないフィールドとヘッダも書かない。

ユーザーは 2026-10-19 に、範囲を下の「2026-10-19 に決めたこと」のとおりに広げることと、ネイティブ化への布石も含めて、効率のよい reuse にすることを求めた。

## 計測 (2026-10-19、試作)

main (9ed9a349) に、VM のコード生成で reuse をまねる使い捨ての試作を足して測った。試作はローカルのブランチ `spike/reuse` のコミット 8ab8efb8 にあり、main には入れない。測ったプログラム (`spike/*.em`)、計測のスクリプト (`spike/measure.sh`、`spike/measure_scripts.sh`) と結果 (`spike/reuse-spike-results.md`) も同じコミットにある。試作は、ブロックの中の `release` と、構築子が1つの配置で `unpack` した値の `decref` を、後ろの同じ物理的な語の数の `con` と組にした。token はフレームのスロットに置き、同じコンストラクタで同じ変数を同じ位置に書くフィールドは書かなかった。`yields` の関数では組にせず、巻き戻しの表には token を入れていない。release ビルドの `eml run` (VM の速いモード) を `/usr/bin/time -l` で3回ずつ測った中央値で、命令の数は `empty` を引いた実行の分である (Apple M4)。

| プログラム | 基準 | 試作 | 減った割合 | 確保 | reuse |
|---|---:|---:|---:|---:|---:|
| `bench/list` | 166,545,584 | 163,393,059 | 1.9% | 200,000 | 100,000 |
| `bench/tree` | 215,818,800 | 196,247,332 | 9.1% | 251,563 | 236,665 |
| `bench/upsert` | 490,278,876 | 490,477,545 | 0.0% | 320,763 | 107,670 |
| `bench/orders` | 261,082,009 | 257,842,856 | 1.2% | 359,320 | 119,490 |
| `bench/iter_pipeline` | 197,891,539 | 201,280,219 | -1.7% | 150,000 | 0 |
| 赤黒木 (Okasaki の `balance c l k r`) | 5,559,286,055 | 5,702,827,034 | -2.6% | 3,824,068 | 67,137 |
| 赤黒木 (`balance` が節を受ける形) | 5,668,384,060 | 5,376,158,223 | 5.2% | 3,953,992 | 3,463,942 |
| レコードの更新を100万回 | 1,355,233,943 | 1,238,264,130 | 8.6% | 1,000,001 | 1,000,000 |
| std の `List.map` と `List.filter` (30万要素を10回) | 3,031,245,639 | 2,798,480,266 | 7.7% | 5,100,002 | 3,471,430 |
| 木の全節を作り直す `incr` (65,535 節を20回) | 1,225,383,286 | 1,120,736,613 | 8.5% | 1,376,235 | 1,310,700 |

S12d のスクリプトは、約 30 MB の入力で命令の数の全体を測った。`wc` 0.3%、`grep -n` 0.5%、`logstat` -0.2%、`logstat --whole` -0.2%、`csv2jsonl` 0.6% で、どれも測定のばらつきの範囲だった。文字列の処理が費用の大半を占めるためである。`logstat` では reuse が 3,653,739 回起きたが、全体の約 286 億命令に埋もれた。

分かったことは次のとおりである。

- reuse 1回あたり、VM の命令は 70〜120 ほど減る。VM では命令の振り分けが費用の大半を占めるので、割合は1桁にとどまる。ネイティブのコードでは確保と解放の割合が大きくなるので、効果はもっと大きくなる見込みである (私の推測)
- レコードの更新では、フィールドがみな借りたまま読まれるので、Perceus は `release` でなく `decref st` を出す。`release` だけを元にすると、reuse は1回も起きない
- `bench/upsert` では `release` が `match compare …` の前にあり、作り直しはそれぞれの腕の中にある。関数の中で動的に数えた reuse の上限は 312,764 回だったが、ブロックの中だけで組むと 107,670 回だった
- Okasaki の書き方の赤黒木は、節を作るのが呼び出し先の `balance` の中なので、`ins` の token が `balance` への末尾呼び出しで使われずに終わる。`balance` が節を受ける形 (FBIP の形) に書けば、9割近くが reuse になる
- `Iter` を重ねる形は、S13e-3 の融合で要素ごとの確保がもう消えているので、reuse の相手がない
- 試作で `into_token` を呼ぶ道と `release_reuse` を呼ぶ道を比べると、`release_reuse` の道のほうが遅かった (`bench/list` で約 15% 増)。どちらも `eml_vm` から見ると crate の外の関数で、`into_token` は `#[inline]` でない。試作の値は `is_unique`、子の `decref`、`into_token` を VM の命令の中に並べた道のものである

## 2026-10-19 に決めたこと

次のことは、ユーザーが決めたか了承した。

- reuse の元に、`release` に加えて、構築子が静的に分かっている値の `decref` を入れる
- token を `switch` の行き先へ渡す
- ネイティブ化への布石も含めて、効率のよい reuse にする

## 目標、目標にしないこと、避けること

目標は次のとおりである。

- レコードの更新、木の作り直し (探索木への挿入、全節の写像)、std の `List.map` と `List.filter` のような、関数の中で手放して作り直す形で、確保と解放をなくす
- 同じコンストラクタで作り直すときは、ヘッダと、変わらないフィールドを書かない
- reuse の判断をすべて LIR に静的に書く。VM は書かれたとおりに `eml_rt` の部品を並べるだけにし、実行時に token を探したり溜めたりしない。ネイティブのコード生成が同じ LIR から同じ命令の列を出せるようにするためである

目標にしないことは次のとおりである。

- 関数をまたぐ reuse (token を呼び出しの引数や戻り値で渡すこと)。Okasaki の書き方の赤黒木はこれが要る。token を関数の外に出さない約束 (S13e の設計の「規則」) を保ち、Lorenzen と Leijen の frame-limited reuse と同じく、token をフレームの中に限る
- token を `jump` で合流するブロックへ渡すこと (下の「token の流れ」)
- 借用パラメータ (ロードマップの「Perceus の最適化」)。借用の推論で、同じ配置の `con` を作る行き先を持つ scrutinee を所有にする規則も、借用パラメータと一緒に入れる
- `drop x` (translate と EP が出す消費の右辺) を reuse の元にすること (私の判断。出る所が少なく、束縛が `()` の右辺を token の定義に変える手間に見合わない)
- 文字列の reuse (`docs/spec/object-model.md` の「種類」のとおり、`String` は token にしない)

避けることは次のとおりである。

- reuse のない関数を遅くすること。reuse の命令を出さない関数のコードと命令の数は、今と同じにする
- token が漏れること。中断の巻き戻しで生きている token と、使われずに終わる道の token は、どれも手放す。checked mode のリークの検査で確かめる
- token が継続のクロージャに入ること。複数回の再開が、同じ箱に2つの値を作るためである (ロードマップの「Perceus の最適化」の約束)

## 制約

- LIR のパスとして、Perceus の後に置く。`eml_lir::Pass` に `Reuse` を足し、`passes_until` が Perceus の後にかける。LIR のパスの順番を知るのは `passes_until` だけである (`docs/spec/lir.md` の「パスの順番」)
- 組は、物理的な語の数と物理的な位置で決める (`docs/spec/object-model.md` の「物理的な形」)。物理的な形の規則は `eml_lir` の1か所にあり、パスと VM のコード生成が同じ規則を使う
- `eml_rt` の reuse の部品 (`is_unique`、`into_token`、`con_at`、`reuse_in_place`、`drop_token`) と、checked mode の token の検査は S9 で入っている。`eml_rt` を変えるときは、`cargo verus verify -p eml_rt` と `verusfmt --verus-only --check` を通す
- 関数のコードは、その関数の LIR だけで決まる。REPL の追記の「同じ名前なら同じコード」の確かめ (`docs/spec/vm.md` の「追記」) を保つためである

## 決めた作り

### LIR の命令

- token は LIR の変数である。Repr は `token` で、RC の対象ではない (`dup`、`decref`、`release` の相手にならない)。extern の行、配置、操作の表には現れない。ネイティブのコード生成では、箱を指すか空を表す1語のレジスタになる
- `release` は、token を定義できる。`release x L #t(..)` の形のまま、token を定義する版を足す (仮に `let r: token = release x L #t(p1, .., pn)`)。意味は、x が一意なら残さない子を手放して箱を token にし、そうでなければ今の `release` と同じに振る舞って空の token を定義する (`docs/spec/object-model.md` の `release_reuse`)
- reuse の元が `decref x` のときは、パスが `release x L #t(_, .., _)` (名前を書く位置のない `release`) に書き換えてから token を定義する。`L #t` は、その道で x のコンストラクタが静的に分かっていることが条件である (下の「reuse の元」)
- `con` は、token を受け取れる (仮に `con@r L #k(args)`)。token が空なら今の `con` と同じく確保する。空でなければその箱を使う
  - その場の書き換え: token の元と同じ配置の同じタグで、`lin` ビットの条件 (元の変数の印が `unr` か、`con` の束縛の印が `lin`。`docs/spec/object-model.md` の「`lin` ビット」) を満たすとき、ヘッダを書かない (`reuse_in_place`)。それ以外は `meta` を書く (`con_at`)。どちらにするかは LIR に書く
  - 書かないフィールド: 引数の変数が、token の元の値を `switch` か `unpack` で分解して束縛した変数で、その物理的な位置が `con` の置く物理的な位置と同じなら、その引数を書かない。走査する語でも生の語でもよい。token の箱は中身を消さないので、その位置には同じ語が残っている (`docs/spec/object-model.md` の「reuse」)。どの引数を書かないかは LIR に書く
- `drop_token r` は、使わなかった token を手放す文である
- テキストの形 (書き方と、印の名前) は作る人に任せる。手書きの LIR のテストが書けるように、`eml_lir::parse` と `pretty` の両方で扱う

### token の流れ

- token は、定義した文から後ろへ、同じブロックの中と、`switch` の行き先 (R3 により入る辺は1本で、引数を持たない) へ流れる。行き先のどれで使ってもよい
- token は `jump` の実引数にならず、合流するブロックへは流れない。`jump`、`return`、`tail` の前に生きている token は、その前で `drop_token` する。合流で token を受けるには、道によって空の token を作る定数が要り、ブロックの引数の検査も増える。`bench/upsert` の LIR (`skew` と `split` は融合で展開されている) を見ると、`release` と `con` は同じブロックか、`switch` の行き先にあり、間に合流はなかった。レコードの更新の `decref` は合流の後にあり、`con` と同じブロックにあった (私の判断)
- token は、呼び出しの引数、閉包の捕獲、`con` のフィールド、`return` の値、extern と `prim` の引数にならない
- token は、yield の地点をまたいで生きない。`yields` の関数の中では、yield しうる呼び出し (`yields` の関数の直接の呼び出し、`yields` の `apply`、再開できる操作の `clause`) の後で生きている token はない。yield しない呼び出し、`Effectful` の extern、`never` の操作の `clause` はまたいでよい (S13e の設計の「規則」)。どの文を yield しうる呼び出しとみなすかは、EP の「yield の地点」(`docs/spec/core-ir.md`) に合わせる。細部は作る人に任せる
- 呼び出しをまたいで生きている token は、その呼び出しを通る中断の巻き戻しで手放す。`eml_lir::unwind::UnwindPoint` に、地点の直後に生きている token の変数を足し、VM の巻き戻しの表がそのスロットを `drop_token` する

### reuse の元

- 元は、Perceus が出した `release x L #t(..)` と、x のコンストラクタがその道で静的に分かっている `decref x` である
- コンストラクタが分かるのは、その文が、x を scrutinee とする `switch` のタグ `t` の case の行き先に支配されているときと、`unpack x` の後にあるときである
- x の Repr は箱のデータ (`obj` か `tobj`) で、コンストラクタはフィールドを持つ。Perceus の `release` は、もとから配置のコンストラクタを名指している
- 生の語の数を含む物理的な語の数は、元の配置のコンストラクタから決まる

### 組の選び方

- 関数のブロックを番号の順にたどり、道ごとに、使える token の集合を持つ。元の文で token を足し、`con` で同じ物理的な語の数の token を1つ取る
- 1つの `con` に使える token が複数あるときは、書かないフィールドが最も多くなる token を選ぶ。同じ数なら、その場の書き換えができる token (同じ配置の同じタグで `lin` ビットの条件を満たす token) を先に選び、それでも決まらなければ最も後に定義した token を選ぶ。`con` は文の順に1つずつ決める (私の判断。S13e の設計で作る人に任せていた「reuse の組の選び方」を、ここで決める)。`bench/upsert` の融合した `split` では、3つの `release` の後に3つの `con` が続く。この規則なら、`t.13` の token が4つのフィールドを書かない `d.54` に、`x.46` の token がどのフィールドも書かない `d.55` に、`x.41` の token が2つのフィールドを書かない `d.56` に行く。最も後に定義した token を先に選ぶ規則では、`x.46` の token が `d.54` に行き、書かないフィールドがなくなる
- どの道でも使われない token の元は、token を定義しない元の文 (`release` か `decref`) に戻す。`into_token` と `drop_token` の2回の操作が、`free_block` の1回より安いことはないためである
- 一部の道で使われない token は、その道で token が使われないと決まる最も早い位置で `drop_token` する。箱を早く返すためである。行き先の入口、`jump`、`return`、`tail` の前、yield しうる呼び出しの前がこれに当たる

### verifier

所有の段 (`verify`) に、token の規則を足す。Perceus の後の LIR にかけている検査で、reuse の後にもかける。

- token の変数は、token を定義する `release` でだけ定義される
- どの道でも、token はちょうど1回使われる。使うのは、token を受ける `con` か `drop_token` である
- token は、上の「token の流れ」で禁じた位置に現れない。yield しうる呼び出しの後で生きていない
- token を受ける `con` の物理的な語の数は、token の元のコンストラクタの物理的な語の数と同じである
- ヘッダを書かない `con` は、元と同じ配置の同じタグで、`lin` ビットの条件を満たす
- 書かないフィールドの引数は、元の値を分解して束縛した変数で、物理的な位置が同じである

### VM

- 命令を3つ足す。token を定義する `release`、token を受ける `con`、`drop_token` である。名前とオペランドの形は作る人に任せる。token はフレームのスロットに置く。スロットの割り当ては、ほかの変数と同じ生存の規則で決まる
- token を定義する `release` は、今の `release` と同じく、形の分かっている部品を命令の中に並べる。x が一意なら、残さない子を `decref` して `into_token` する。そうでなければ、今の `release` の道 (`is_last` の判定) を通り、空の token をスロットに書く
- token を受ける `con` は、token が空なら `alloc_data` で確保してすべてのフィールドを書く。空でなければ、LIR の指示に従って `reuse_in_place` か `con_at` で箱を得て、書かないフィールドを飛ばして書く
- 巻き戻しの表の行に token のスロットを足し、巻き戻しで `drop_token` する (`docs/spec/vm.md` の「巻き戻し」)
- `RunStats` に、token の箱を使った `con` の回数を足す。checked mode でだけ数える。欄の名前は作る人に任せる (S13e の設計の「`RunStats` に足す欄」)
- `eml_rt` の `into_token` と `drop_token` に `#[inline]` を付ける。VM は crate の外にあり、付けないと速い道が crate をまたぐ呼び出しになるためである (`docs/spec/object-model.md` の「核の操作」の `#[inline]` の理由と同じ)。試作の計測で、crate をまたぐ呼び出しの費用が reuse の利益を打ち消しうると分かった。付けた後の値を `bench/rt.sh` と `bench/run.sh` で確かめる

### ネイティブ化への布石

上の作りのうち、次の点はネイティブのコード生成のための決め方である。

- reuse の判断 (組、その場の書き換え、書かないフィールド、使わない道の `drop_token`) を、すべて LIR に書く。VM とネイティブのコード生成は、同じ LIR を同じ部品の列に下ろすだけになる
- token は1語の変数なので、ネイティブではレジスタかフレームの1語に置ける。実行時の溜め置きや探索はない
- `docs/spec/lir.md` の「実行の意味」に、token を定義する `release`、token を受ける `con`、`drop_token` を、`eml_rt` の部品の列として書く。VM はその列のとおりに動き、ネイティブのコード生成も同じ表に従う
- 巻き戻しの地点ごとの token の一覧は、ネイティブの巻き戻しの表にもそのまま使える

## 完了の条件

基準は S13e-3 の終わり (9ed9a349) の値である。

- `bench/` に、上の計測の表の次の4つの形を足す。レコードの更新 (`spike/records.em`)、`balance` が節を受ける赤黒木 (`spike/rbtree_fbip.em`)、木の全節の写像 (`spike/tree_map.em`)、std の `List.map` と `List.filter` (`spike/std_list.em`) である。試作のプログラムを写し、要素の数は `crates/eml_vm/tests/bench.rs` が checked mode で無理なく流せる大きさに縮める。名前と大きさは作る人に任せる。`bench.rs` が、足したプログラムの `RunStats` を固定する
- `bench/list`、`bench/tree`、`bench/upsert`、`bench/orders` と足した4つで、`RunStats` の reuse の回数が 0 より大きい
- `bench/tree` と足した4つで、`bench/run.sh` の命令の数 (`empty` を引いた実行の分) が、試作と同じかそれ以上に減る。比べ方は、同じ `bench/` のプログラムを、基準 (9ed9a349)、試作 (8ab8efb8 を、足したプログラムが入るように main の上に載せたもの)、実装の3つで測り、基準からの減った割合を比べる。試作の表の割合は大きい入力の値なので、そのままの目標にはしない。届かないプログラムがあれば、理由を `benchmarks.md` に書き、ユーザーに確かめる
- `bench/upsert` の reuse の回数が、試作のブロックの中だけの値 (107,670) より多い。関数の中の動的な上限 (312,764) との差と、その理由を `benchmarks.md` に書く
- reuse の命令を1つも出さないプログラムの命令の数は、基準から 0.5% を超えて増えない
- `bench/scripts.sh` を流し、`logstat` の実時間と最大の常駐メモリを記録する。遅くならないことは、実時間でなく命令の数で確かめる。`bench/scripts.sh` と同じ入力と引数で、`/usr/bin/time -l` の `instructions retired` の3回の中央値 (全体の命令の数) を基準と実装で測り、`logstat` と `logstat --whole` が 1% を超えて増えない。実時間は2桁の秒でしか出ず、1% の差はばらつきに埋もれるためである。測り方は `spike/measure_scripts.sh` と同じで、リポジトリの道具に足すかは作る人に任せる
- checked mode で、中断が生きている token を通る UI テスト (下の「足すテスト」) がリークなしで通る
- `bench/rt.sh` の各計測が、`#[inline]` を付ける前から 0.5% を超えて増えない

## テストの変更

### 種類1 (通過と失敗の変更)

今の時点で分かっているものはない。動作は変えないので、UI テストと REPL のトランスクリプトの出力は変わらない見込みである。実装の途中で見つかったら、作る人はコミットの前にこの節に1つずつ足し、理由を書いて、ユーザーの確認を受ける。

### 種類2 (期待値の変更)

次の範囲の期待値は、reuse のパスに合わせて書き換えてよい。理由は、Perceus の後に reuse のパスが入り、VM に reuse の命令が入ったことである。

- `crates/eml_core_ir/tests`、`crates/eml_lir/tests`、`crates/eml_vm/tests` の、パイプラインの終わりまでの LIR と逆アセンブルのスナップショットのうち、token を定義する `release`、token を受ける `con`、`drop_token` が入るもの。Perceus を直接呼ぶテストの期待値は変わらない
- `crates/eml_vm/tests/bench.rs` と `crates/eml_vm/tests/scaling.rs` の `RunStats` のうち、確保、RC、reuse の回数が変わるものと、足した欄
- 試作で変わった `crates/eml_vm/tests/codegen.rs` の `a_dead_switch_scrutinee_does_not_share_a_slot_with_the_case_fields` のように、token のスロットでフレームの大きさやスロットの番号が変わる逆アセンブルの期待値

`specialization_matches_hand_written_work` と `specialization_matches_hand_written_rc` の組が、reuse の回数の違いで確保か RC の回数を違えるようになったら、作る人は原因を調べてユーザーに確かめる。テストを直すのは、その後である。

### 足すテスト

- `crates/eml_lir/tests` に、reuse のパスを手書きの LIR で確かめるテスト。同じ物理的な語の数の組、違うコンストラクタの組 (`meta` を書く)、その場の書き換えと書かないフィールド、`lin` ビットの条件でその場の書き換えをしない形、`decref` を元にする形とコンストラクタが分からない `decref` を元にしない形、`switch` の行き先の片方で使い片方で `drop_token` する形、`jump` の前の `drop_token`、どの道でも使わない元を戻す形、yield しない呼び出しをまたぐ形、yield しうる呼び出しをまたがない形、1つの `con` に token が2つあるときの選び方
- verifier が token の誤用を断るテスト。2回の使用、使わない道、呼び出しの引数と `jump` の実引数と `return` の値と閉包の捕獲に現れる token、yield しうる呼び出しをまたぐ token、語の数の違う `con`、条件を満たさないその場の書き換え、位置の違う書かないフィールド
- `crates/eml_vm/tests` に、reuse の命令の実行を確かめるテスト。一意な箱の使い回し、共有された箱で空の token になって確保する道、書かないフィールドが元の値を保つこと、checked mode の `RunStats` の reuse の回数
- `crates/eml_lir/tests` か `crates/eml_vm/tests` に、巻き戻しの地点の token の一覧を確かめるテスト
- UI テスト (`tests/ui/run/`) に、token が生きている間に中断する形。`try_io` が捕まえる IO の失敗と、`exit` の2つで、checked mode のリークの検査を通す。試作では `tests/ui/run/unwind/io_extern_as_value.em` で token が1つ漏れた
- Miri: `eml_rt` に新しい FFI の境界は入らないので、足さない

## 文書の更新

- `docs/spec/lir.md`: 「パスの順番」に reuse を足し、「reuse analysis と借用パラメータの最適化は後で追加する」を直す。token の Repr、token を定義する `release`、token を受ける `con`、`drop_token`、token の流れの規則、組の選び方、verifier の検査、テキストの形、「実行の意味」の部品の列を書く
- `docs/spec/vm.md`: reuse の命令と、巻き戻しの表の token のスロットを書く
- `docs/spec/object-model.md`: 「reuse」の節の「後の reuse analysis」を、入った reuse のパスへの参照に直す。`into_token` と `drop_token` の `#[inline]` を「核の操作」の一覧に足す
- `docs/implementation/architecture.md` と `CLAUDE.md` (英語のまま): LIR のパスの列に reuse を足す
- `docs/implementation/status.md`: 関数をまたぐ reuse をしないこと (Okasaki の書き方の赤黒木) と、合流をまたぐ token がないことを、制限として書く
- `docs/implementation/benchmarks.md`: 「S13e-4」の節に、`bench/run.sh`、`bench/scripts.sh`、`bench/rt.sh` の値と、上の計測の表 (試作の値) を書く
- `docs/future/roadmap.md`: S13e-4 を「済み」にし、S13e を完了にする。「Perceus の最適化」の reuse の項目を、入れたものへの参照に直す。関数をまたぐ reuse と合流をまたぐ token を、残りの項目として書く
- S13e の設計 (`2026-10-18-s13e-specialization-design.md`) の「S13e-4 reuse analysis」の節の頭に、この文書に代わったことを1行で書く

## 作る人に任せること

- テキストの形と、命令と印の名前
- token の Repr を `eml_extern::Repr` の値にするか、`VarInfo` の別の欄で表すか
- パスのモジュールの分け方と、道ごとの token の集合の持ち方
- yield しうる呼び出しの見分け方の細部
- VM の命令のオペランドの形と、`RunStats` の欄の名前
- 足す `bench/` のプログラムの名前と大きさ
- 計画のタスクの分け方
