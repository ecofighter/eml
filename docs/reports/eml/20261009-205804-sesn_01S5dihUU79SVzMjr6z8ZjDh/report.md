# eml: VMベースインタプリタとネイティブコード生成の土台の調査

対象は commit `97277f19a4d7f514dd64008af3c1391507c52322` の `eml`。コードの引用はこのコミットの行番号である。ここでの「検証済み」はコードを読むか実行して確かめたこと、「推測」は読んだ事実からの私の推論、「資料の主張」は論文や公式ページの記述を指す。

## 要約

現在の設計の骨格は変えなくてよい。足りないのは新しいIRの層ではなく、ホットパスの実装、最適化パス、効果の下げ方 (lowering)、バイトコードへの出力である。七つの問いへの答えは次のとおり。

| 問い | 結論 | 確度 |
|---|---|---|
| HIRを再設計すべきか | しない。HIRは構文に忠実な名前解決済みの木で、IDEと型検査の入力として適切である。バックエンドの都合で形を変える理由は見つからなかった | 高 |
| HIRとCore IRの間にIRを追加すべきか | 今は追加しない。追加する条件を「改善提案」に置いた | 中 |
| Core IRを再設計すべきか | 全面的な再設計はしない。前向きの辺だけの基本ブロック列、ブロック引数、Repr、box/unbox、明示的なRCという核は、Lean や Cranelift の設計と一致する。直すのは `saved` の持ち方、verifierの段、ループの辺の三点 | 中から高 |
| `simplify` と `perceus` は現在のCore IRに対するパスでよいか | Perceus はよい。`simplify` という名前のパスは既に無く、translate 内の書き換えと `contract` に置き換わっている。新しい最適化パスを入れるなら、Core IR の上で translate と box の挿入の間に置く | 中から高 |
| Core IRの下にLIRを追加すべきか | 独立したIRとしては追加しない。効果を持たない形に下げた Core IR (lowered Core) を verifier の段として定義し、そこからバイトコードか Cranelift へ1回の線形な走査で出力する | 中 |
| 各IRに最適化パスを追加すべきか | HIRには追加しない。Core IR には単相化、既知の呼び出しへの置換、インライン化、jump threading を入れる。Perceus の後には RC の最適化 (借用、再利用) を入れる。バイトコードには融合命令を入れる | 中から高 |
| 現在のCore IRを実行するインタプリタを削除してVMへ移行すべきか | 削除しない。VMを新しい実行系として足し、CEK は意味論の参照実装と差分テストの基準として残す。設計文書の方針とも一致する | 高 |
| VMはLIRを実行するか、専用の命令列に下げるか | 専用の命令列に下げる。ただし中間に新しいIRは挟まず、lowered Core から直接、レジスタ型のバイトコードを作る | 中 |

最も根拠が強い発見は性能の測定である。`eml run` のホットパスは、命令の解釈よりも確保と解放に費やされている。`fib 22` を callgrind で測ると、実行命令の約53%が malloc、free、引数と退避用 `Vec` の組み立てだった。コピーの上で2か所を直すだけ (extern の引数を固定長の配列に、環境の `Vec` を使い回す) で、`fib 22` の実行命令数は179,075,838から112,677,511に減った (約37%減)。`fib 30`、1000万回のループ、`handle` を使う100万回の再帰の中央値は、それぞれ36%、51%、27%短くなった。この結果は、VMへの移行より前にやるべき作業があること、またVMの利得を評価する基準線が低すぎることを示す。

効果の下げ方は、バイトコードVMとネイティブの両方に影響する。現在の継続は、ヒープ上の `Frame::Return` の連結リストで、非末尾の呼び出しのたびに確保される。設計文書は既に evidence passing (Koka 方式) を選んでいる。これを Core IR から Core IR への変換にすれば、VM は継続の捕捉を持たずに済み、連続したスタックで呼び出せる。このため、VMは evidence passing と同時か直後に作るのがよい。それより前に作る場合は、現在の `Runtime` を使い回す差し替え型にとどめる。

## 調査結果

### 0. 依存と版 (検証済み)

- 版の根拠は `Cargo.lock` と `Cargo.toml` である。主な依存は `logos` 0.16.1、`rowan` 0.16.1、`la-arena` 0.3.1、`text-size` 1.1.1、`ariadne` 0.6.0、`clap` 4.6.7、`insta` 1.49.0で、edition は 2024 である (`Cargo.toml:7`)。バックエンドの依存 (Cranelift、LLVM など) は無い。ランタイムは自前のアリーナである (`crates/eml_runtime/src/heap.rs`)。測定に使った `rustc` は 1.97.0 である (サンドボックスのもの。`flake.nix` が指す版とは一致を確かめていない)。
- フロントエンドの crate (`logos`、`rowan`、`la-arena`) は本調査の問いに関わらないので、版ごとの既知の問題は調べていない。
- バックエンド側の外部資料は、コードが依存しない対象の最新の記述である。Cranelift は eml が使っていないので、Wasmtime の main の文書と記事を読んだ。インライナは Wasmtime 36 で入り、既定では無効と記事が述べる (記事のとおり)。実際に採用するときは、その時点の Cranelift の版で `cranelift-codegen` と `cranelift-frontend` の API と制約を確かめる必要がある。Lean の記述は upstream の commit 6562e9d (2026年9月の nightly 付近) と 4.22.0 のリリースノートに基づく。

### 1. パイプラインの現状 (検証済み)

実行の流れは `eml_cli::Session` が組む。HIR (`eml_hir`) は名前解決済みの木で、型は別の側の表 `TypedProgram` が持つ。Core IR への変換 (`translate`) は型を読む唯一の段で、続く4つのパスを `crates/eml_core_ir/src/pipeline.rs:48` の `lower_until` が順に流す。

```
source → CST → HIR (ItemTree / Body、アリーナ) ─┐
                 型検査 → TypedProgram (側の表)   ├→ translate → boxing → contract → perceus → CEK
                                                  ┘   (各パスの後に verifier、debug ビルドのみ)
```

- Core IR は、関数ごとの基本ブロックの列である (`crates/eml_core_ir/src/lib.rs:127`)。ブロックは引数、文の列、終端を持つ。文は `let`、`unpack`、`dup`、`decref`、`release` で (同175行)、終端は `return`、`tail`、`jump`、`switch` である (同238行)。右辺には `call`、`apply`、`perform`、`resume`、`handle`、`closure`、`con`、`extern`、`box`、`unbox` がある (同351行、442行)。
- 辺はすべて番号の大きいブロックへ向かい (R2、`docs/spec/core-ir.md:57`)、ループの辺は無い。ループは末尾呼び出しで表す。
- 変数ごとに Repr (`obj`、`tobj`、`int`、`enum`、`unit`) を持ち、型は持たない。型変数のフィールドは `tobj` で、多相な位置の `Int` は `box` / `unbox` で変換する (`docs/spec/core-ir.md:137` の位置の規則)。
- Perceus は `dup` / `decref` / `release` を挿入し、呼び出しごとに `saved` を埋める (`crates/eml_core_ir/src/perceus.rs:14`、`crates/eml_core_ir/src/lib.rs:357`)。
- `simplify` というパスは存在しない。`docs/implementation/architecture.md:222` が、旧 `simplify` の書き換えを translate が組み立てる時点で行うようになり、`simplify` は書き換えのたびに枝の部分木を置き換えて長い `else if` の連鎖で2乗の時間がかかった、と記録している。今あるパスは box の挿入、`contract` (使われない純粋な `let` の削除と末尾呼び出しの形成、`crates/eml_core_ir/src/contract.rs:9`)、Perceus だけである。

### 2. HIR

- HIRは `Program` の中のモジュールと、関数ごとの `Body` (式、パターン、局所変数のアリーナ) からなる (`crates/eml_hir/src/hir.rs:190`)。ノードは `TextRange` を持ち、誤りの跡として `Missing` を残す。`Block` の `last_start` のように診断の fix のためだけにある欄もある (同381行付近)。これは構文に忠実でIDE向きの形で、バックエンド向きの形ではない。
- 型の情報は `BodyTypes` の側の表にある。式の型、パターンの型、参照ごとの具体化 (`instantiations`)、呼び出しの矢印ごとの `mask` が、`ExprId` で引ける (`crates/eml_types/src/lib.rs:101`)。
- 評価の順と引数をまとめて渡す範囲は、HIR の `eval.rs` の `call_steps` だけが持つ (`crates/eml_hir/src/eval.rs:19`)。持ち越しのパス (`crates/eml_types/src/carry.rs:89`) と translate (`crates/eml_core_ir/src/translate/expr.rs:281`) が同じ関数を読む。同じ規則を2か所に書くとずれて持ち越し規則が実行と食い違うので1か所にした、という意図がファイルの冒頭にある (`crates/eml_hir/src/eval.rs:1`)。これは健全な設計で、重複は見つからなかった。
- 継続 `k` の使い方 (`Body::continuations`、`crates/eml_hir/src/hir.rs:204`) も HIR が持ち、translate が直接の形か包む形かを決める (`crates/eml_core_ir/src/translate/mod.rs:67`)。

HIR を作り直す動機になりうるのは、型検査の結果を式に埋め込めないことだが、これはインクリメンタル化と LSP のために側の表にした判断であり (`docs/future/roadmap.md:293` の処理系の節、同324行の source map の項目)、バックエンドの要件とは衝突していない。

### 3. translate と Core IR の設計

- translate は6ファイル2,848行で、次の仕事を1つの段で行う。型から Repr を決める、`instantiations` を読んで `==` の比べ方を選ぶ、`mask` を付ける、ラムダ・節・handle の本体を関数に持ち上げる、`k` の形を決める、eval/apply の場合分け (`saturate`、`crates/eml_core_ir/src/translate/expr.rs:105`)、決定木の作成と case-of-case (`crates/eml_core_ir/src/translate/pattern.rs:458`)、ブロックの組み立て。これは設計文書が選んだ構成で、translate を「型に依存する唯一の段」にする方針である (`docs/superpowers/specs/2026-10-07-redesign-design.md:109`)。
- 型変数のフィールドは具体化した型を見ない `tobj` になる。`data Pair a = Pair a Int` は `Pair(tobj, int)` で、`Option Int` と `Option a` は同じ配置を使う (`docs/spec/core-ir.md:122` のデータの配置)。単相化は行わない。実際に `List Int` を作る `range` は、`box lo.0` の後に `con List #1(lo.6, t.4)` を作る形になる (`tests/ui/run/data/list.em` を Perceus まで下げて確かめた)。ロードマップは box の挿入の試作で、型変数の位置の `Int` の変換が UI で302,149回、CPS のラムダで200,125回あったと記録している (`docs/future/roadmap.md:298`)。
- 捕まえた変数のないラムダは `Atom::Fn` の定数になり、クロージャを確保しない (`crates/eml_core_ir/src/lib.rs:539` 付近)。このため、ラムダ持ち上げ後の Core IR でも「引数に渡された関数定数を直接呼びに置き換える特殊化」が書ける (推測)。
- 算術と比較は `extern Prelude.+` などの `Rhs::Extern` になる。`fib` の Core IR では、ループの中身のほとんどが `extern` である。`Extern` の表は引数と結果の Repr と純粋性 (`Pure`、`MayFail`、`Effectful`) を持つ (`crates/eml_extern/src/lib.rs:18`)。

### 4. 最適化パスの現状

- translate の内部に、既知のコンストラクタへの `switch` の解決、case-of-case、返すだけの合流ブロックの転送がある (`docs/implementation/architecture.md:222` 以降の translate の節)。
- 未実装なのは、インライン化、jump threading、借用パラメータ、reuse、ループ化である。設計は文書にある。インライン化と呼び出しを作る変換は translate と box の挿入の間に置き、`tail`、`box`、`unbox` を出さないことが決まっている (`docs/spec/core-ir.md:292`、`docs/future/roadmap.md:317`)。
- Perceus 後のIRを変換するパスは、RC の命令の釣り合いを壊さないことが前提になる。verifier の所有の段 (R6、R7) が各辺の所有の多重集合を比べるので、壊せば検出される。

### 5. 現在のインタプリタ (CEK) と性能

- 構成は `machine.rs` (制御、環境、`jump`、`switch`)、`runtime.rs` (ヒープ、継続、関数値の適用、extern)、`effects.rs` (`perform` と `resume`)、`externs.rs` の約1,360行である。`Machine` が IR の形に依る部分だけを持ち、`Runtime` が `Transfer` で行き先を返す。将来のバイトコードVMが `Runtime` を使い回せるように分けた、と書かれている (`docs/implementation/architecture.md:204` 以降の「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」)。戻りのフレームの再開の番地を `u64` にしたのも、VMの `pc` を入れるためである (`crates/eml_interp/src/machine.rs:305`)。
- 呼び出しごとの確保 (検証済み)。`Rhs::Extern` は引数を `Vec` に集めてから `call_extern` を呼ぶ (`crates/eml_interp/src/machine.rs:113`、`crates/eml_interp/src/runtime.rs:77`)。関数に入るたびに `Env::new` が `Vec<Option<Value>>` を確保する (`crates/eml_interp/src/machine.rs:259`)。非末尾の呼び出しは `save` が `Vec<(u32, Value)>` を作り (`crates/eml_interp/src/runtime.rs:83`)、`push_return` がヒープに `Frame::Return` を確保し (同204行)、戻るときは `Env::restore` がもう一度 `Vec` を確保する (`crates/eml_interp/src/machine.rs:293`)。
- 継続はヒープ上の不変なフレームの連結リストで、`perform` は handler と `Mask` のフレームだけの連鎖をたどる (`crates/eml_interp/src/effects.rs:11`)。`multi` の継続は再開のたびに区間を写す (`crates/eml_runtime/src/heap.rs:433`)。
- 測定 (この環境、release ビルド、各3回の中央値、実測値はサンドボックスの揺れを含む)。

| プログラム | 元のコード | 2か所を直したコピー | 変化 |
|---|---|---|---|
| `fib 30` | 0.906 s | 0.583 s | −36% |
| 1000万回の末尾再帰 (`loop`) | 2.435 s | 1.194 s | −51% |
| `handle` 中の100万回の再帰 (`state`) | 0.886 s | 0.645 s | −27% |
| `fib 22` の実行命令数 (callgrind) | 179,075,838 | 112,677,511 | −37% |

  直した2か所は、`Rhs::Extern` の引数を長さ4の固定配列に集めることと、`Env` の `Vec` を `clear` と `resize` で使い回すことである。コピーの上で `cargo test -p eml_cli --test integration ui::` が通ることも確かめた。差分と測定に使ったプログラムは `experiments/` に置いた。
- 元のコードの `fib 22` の内訳では、`_int_free` が11.5%、引数の `Vec` を作る `from_iter` が9.8%、同じ経路の `try_fold` が9.8%、`malloc` が9.5%、`free` が5.9%、`Option<Value>` の `from_elem` が2.7%、`save` が2.0%、`__rdl_alloc` が1.9% で、合計は約53%だった。

### 6. 継続とエフェクトがVMの設計に課す制約

- `multi` の継続は何度でも再開でき、再開のたびに捕まえた区間を写す (`docs/spec/effects.md:29` の継続の多重度)。持ち越し規則により、`multi` をまたいで持てるのは `Unr` の値だけである。`once` の継続は `Lin` で、`File` のような線形な値を持てる。
- 継続の区間に入るのはフレームが退避した値だけである。インタプリタの `Value` は自分の種類を持つので (`crates/eml_runtime/src/heap.rs:17`)、区間を写すときは参照の値だけを `dup` すればよい。スタックの値に型の情報がないネイティブでは、この方法は使えない (推測)。
- 設計文書は、ネイティブのエフェクトを generalized evidence passing と yield bubbling で実装し、Core IR から Core IR への変換パスにすると決めている。CEK は直接の意味論の参照実装として残し、差分テストの基準にする (`docs/future/evidence-passing.md:95`、`docs/future/roadmap.md:300`、`docs/superpowers/specs/2026-10-07-redesign-design.md:57`)。必要な前提として、Core IR に row の情報を持たせること、ラベルの順序を決めることが挙がっている。

## 技術的背景と関連研究

| 技法 | 出どころ | eml の実装との関係 |
|---|---|---|
| Perceus (正確な参照カウントと再利用) | [Reinking ら, PLDI 2021](https://dl.acm.org/doi/10.1145/3453483.3454032) | `dup` / `decref` の挿入はこれに従う。`switch` が scrutinee を消費せず、行き先の入口で `release` に置き換える形は、drop specialization に当たると推測する (その節は未読)。reuse 解析は未実装 |
| 借用パラメータと λRC | [Ullrich と de Moura, IFL 2019 (arXiv)](https://arxiv.org/abs/1908.05647) | 借用の推論は未実装。末尾呼び出しを壊す降格を、輪の上の仮引数を所有にして避ける方針は同論文と一致する |
| 参照カウントの再利用の見直し | [Lorenzen と Leijen, ICFP 2022](https://dl.acm.org/doi/10.1145/3547634) | 設計文書の `release` を `release_reuse` に書き換える案は、同論文の drop-guided reuse (Perceus の後で drop を書き換える) と同じ方向 |
| join point を持つ直接形の IR | [Maurer ら, PLDI 2017](https://dl.acm.org/doi/abs/10.1145/3062341.3062380) | join point は、ブロック引数を持つ飛び先に当たる (推測)。Lean の IR も join point と `jmp` を持つ (Beans 論文) |
| ブロック引数 (φ の代替) | [Cranelift IR の文書](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md) | Core IR のブロック引数と `jump` は Cranelift のブロック引数と1対1に写る (`docs/spec/core-ir.md:10`) |
| eval/apply | [Marlow と Peyton Jones, JFP 2006](https://www.cambridge.org/core/journals/journal-of-functional-programming/article/making-a-fast-curry-pushenter-vs-evalapply-for-higherorder-languages/02447DB613E94DC35ACDCB24DB39F085) | `Call::Apply` の足りない・ちょうど・余るの場合分けは eval/apply。同論文は、コンパイルする実装には eval/apply を使うべきだと結論している (要旨) |
| 決定木へのパターンマッチのコンパイル | [Maranget, ML Workshop 2008](https://dl.acm.org/doi/10.1145/1411304.1411311) | 最初の行でコンストラクタ、タプル、リテラルのどれかのパターンを持つ最も左の列を選ぶ規則 (`docs/spec/core-ir.md:210` の `match` の項、`crates/eml_core_ir/src/translate/pattern.rs:458` の `decide`) は、同論文が比較する従来の経験則のうち、最初の行に基づくものに近い (推測)。`docs/future/roadmap.md:326` が列の選び方の見直しを保留にしている |
| generalized evidence passing | [Xie と Leijen, ICFP 2021](https://dl.acm.org/doi/10.1145/3473576) | `docs/future/evidence-passing.md:20` 以降の方式の選択が採用する方式。Koka の C バックエンドで使われる |
| fiber とスタックの切り替え | [Sivaramakrishnan ら, PLDI 2021 (arXiv)](https://arxiv.org/abs/2104.00250) | 設計文書が方式 B として残している案。fiber のスタックは小さく始め、足りなければコピーして倍にする |
| 表現を要するスカラーの box 化 | [Lean の `LCNF/ExplicitBoxing.lean` (upstream、commit 6562e9d)](https://github.com/leanprover/lean4/blob/6562e9dfa09e4e9b1839dbabbdca26dcea1b91f9/src/Lean/Compiler/LCNF/ExplicitBoxing.lean) | `boxing.rs` の `f$boxed` と一様化は、Lean の `_boxed` 版に対応する。Lean の同ファイルは、`_boxed` 版を「インタプリタとクロージャの確保で使う」と書き、`_boxed_const` の補助定義も作る (roadmap の `_boxed_const` と同じ名前)。同ファイルは `inc` / `dec` を扱わない、つまり RC の挿入より前に走る |
| レジスタ型VM | [Shi ら, TACO 2008](https://dl.acm.org/doi/10.1145/1328195.1328197) | Java のスタックコードをレジスタコードに変換した評価で、実行する VM 命令を平均46%以上減らした (要旨)。eml の Core IR は最初からレジスタ型に近いので、この変換が要らない |
| copy-and-patch | [Xu と Kjolstad, OOPSLA 2021](https://arxiv.org/abs/2011.13127) | バイトコードから機械語への低コストな変換。将来の JIT の選択肢 (推測のみ) |

### 新しい研究 (2022年以降)

| 論文 | 内容 (読んだ範囲) | eml との関係 |
|---|---|---|
| Lorenzen, Leijen, Swierstra. "FP²: Fully in-Place Functional Programming". ICFP 2023 (PACMPL 7, ICFP, Article 198). [DOI](https://doi.org/10.1145/3607840) | 引数が共有されていなければ、確保なし・定数スタックで実行できる関数を静的に検査する線形な FIP 計算を与え、Koka に実装した (要旨と本文の抜粋)。計算は借用パラメータと unbox した組を含む | eml の `release` と reuse の計画の理論的な到達点。`Lin` と `multi` の組み合わせで同じ保証が成り立つかは未検討 |
| Lutze, Schuster, Brachthäuser. "The Simple Essence of Monomorphization". OOPSLA 2025 (PACMPL 9, OOPSLA1, Article 116). [DOI](https://doi.org/10.1145/3720472) | 型の流れの解析で単相化する方法。高階の多相や存在型にも広がり、多相再帰は循環する流れとして検出する (要旨と抜粋) | 提案4の多相再帰の扱い (上限を超えたら一様な版に戻す) の理論的な裏づけ。eml の型付けが高階の多相を許すかは確認していない。許さないなら、必要な範囲はもっと小さい |
| Gaißert, Bolz-Tereick, Brachthäuser. "Tracing Just-in-Time Compilation for Effects and Handlers". OOPSLA 2025 (PACMPL 9, OOPSLA2, Article 307). [DOI](https://doi.org/10.1145/3763085) | Eff、Effekt、Koka を共通のバイトコードに下げ、RPython の tracing JIT で実行する評価。呼び出しスタックを「スタックのスタック」で表し、ハンドラを探す。Koka は大域の evidence vector を持つ (要旨と抜粋) | バイトコードに効果を直接載せる別案の実測例。eml の VM を evidence passing の後に作る案との比較対象になる。RPython を使う前提は eml に合わない |
| Ma, Jung, Zhang. "Virtualizing Continuations". PACMPL 2026. [DOI](https://doi.org/10.1145/3808289) | スタックに置いた資源を持つ lexical effect handler で、スタックのコピーが参照を無効にする問題を、スタック領域の仮想化で解く。multishot を完全にサポートする (要旨のみ) | eml のフレームと値はすべてヒープの RC であり、この問題は起きない。スタックのコピー方式を採る場合の落とし穴の参考 |
| Lorenzen, Leijen. "Reference Counting with Frame Limited Reuse". ICFP 2022 | 前掲 (要旨のみ) | 前掲 |

これらの結果が eml に移る条件は、(1) eml のプログラムの負荷が各論文の評価と近い構造 (データ構造の更新、効果の使い方) を持つこと、(2) 評価の実装 (Koka、RPython、Effekt) が eml の Core IR と同じ前提 (一階、RC、型消去) を持つことである。どちらも未確認である。

### 各技法との差と、それぞれの後の研究

- **Perceus**。論文は「明示的な制御フローを持つ関数型のコア言語」から始める (要旨)。eml の Core IR は制御フローが明示的なので、前提に合う。eml の `switch` はフィールドを借りて始め、行き先で `dup` か `release` を置く (`docs/spec/core-ir.md:311` 以降の Perceus の節)。論文の match の規則との対応は、本文を読んでいないので確認していない。後続の研究は、reuse 解析が小さな変形に弱く、ピークのヒープ使用量を際限なく増やしうることを指摘し、Perceus の後で drop を書き換える方式を提案した (Lorenzen と Leijen 2022、要旨)。eml の設計文書の reuse の入れ方はこの方式に合う。注意点は、reuse のトークンを `saved` に入れないという設計文書の約束である。複数回の再開が同じ箱に2つの値を作るためで、`multi` の継続を持つ eml 固有の条件である。
- **借用パラメータ**。Ullrich と de Moura は、借用の推論の初期値をすべて借用にして不動点まで所有に変え、末尾呼び出しの引数が所有なら呼ばれる側の仮引数も所有にする、と述べている (本文を読んだ)。eml の `docs/future/roadmap.md:302` 以降の計画は同じ規則で、`ownParamsUsingArgs` を名指ししている。論文はさらに、`pap` で部分適用される定数は借用を持てないので所有版のラッパーを作ると述べる。eml の `f$boxed` と同じ考え方で、借用を入れるときは `f$boxed` との合成も考える必要がある (推測)。
- **evidence passing**。論文は、EPS が「すべての effect handler プログラムで動く」点で先行の EPT と違う、と述べる (本文の抜粋)。eml の第一級の `k` と `multi` にはこの一般性が要る。yield のたびに、効果を持つ関数の呼び出しの後で「yield 中か」を確かめる費用は、現代のプロセッサでは安く、予測もしやすいと論文は述べる。評価は Koka の C バックエンドと Haskell のライブラリで、multicore OCaml などと比べたものである。この結果が eml に移るには、eml が生成する Core IR の呼び出しの形と継続のクロージャの大きさが Koka と近いこと、`Lin` の値の破棄 (`never` の中断) を yield の経路で正しく行えることが要る。後者は設計文書も未決の論点に挙げている (`docs/future/evidence-passing.md:102` 以降)。
- **単相化と FP²**。上の表の2論文を参照。単相化の多相再帰の扱いは、eml の型付けの範囲が小さければ「循環する流れを検出したら一様な版」で足りると考える (推測。型付けの範囲は未確認)。FP² の `fip` のような静的な検査を eml に置くかは、`multi` の継続が同じ値を共有しうる点との相性を確かめる必要があり、未検討である。
- **VM**。Shi らの結果は Java VM のスタックコードをレジスタコードにした評価で、eml にそのまま移せる数値ではない。移るための条件は、eml のバイトコードのディスパッチ費用が現在の `Core IR` を歩く費用より支配的であることだが、上の測定では確保が約53%を占めるので、まず確保を除くべきである。

## 改善提案

各問いへの回答と設計を先に述べ、その後に優先順位つきの提案を並べる。

### Q1 HIRを再設計すべきか

しない。理由は三つある。HIR の欄の多く (`TextRange`、`Missing`、`last_start`、`has_errors`) は診断と LSP のためにあり、これらを取り除くとフロントエンドの要件を満たせなくなる。評価の順、`k` の使い方、捕獲の計算といったバックエンドに必要な導出は、HIR 側の関数 (`call_steps`、`known_arity`、`continuations`) に1か所ずつ置いてあり、型検査と translate が共有している。型は側の表にあるので、HIR を書き換えても型検査の結果は無効にならない。

変えるとすれば、既に計画にある「位置を source map に移す」だけにする (`docs/future/roadmap.md:324`)。インクリメンタル化に載せ替えるまで利点がないので、それまでは触らない。

### Q2 HIRとCore IRの間にIRを追加すべきか

今は追加しない。追加の候補だった「型付きで評価順が明示された木」が必要になる場面を、現在のコードに当てはめて確かめた。

- 単相化と Eq/Ord/Show の特殊化は、(関数、型引数) の組を鍵にして translate を呼び直せば実現できる。translate が型に依存する唯一の段という方針と合う。IRを足す必要はない。
- 高階関数の特殊化は、ラムダ持ち上げ後の Core IR で書ける。捕まえた変数のないラムダは `Atom::Fn` の定数なので、引数に定数が渡る呼び出しは、呼ばれる関数の複製と `Apply` から `Direct` への置換で扱える。捕獲があるときは、`MakeClosure` の捕獲を追加の引数にする。Lean は `@[inline]` と `@[specialize]` の属性で、抽象を重ねたコードの負担を除いている ([Lean 4 の論文](https://lean-lang.org/papers/lean4.pdf))。
- evidence passing に必要な row 情報は、設計文書が Core IR に持たせる案を選んでいる。

再検討の条件は二つにする。一つは、Core IR に型か row を持たせる必要が2つ以上のパスで生じたとき。型を Core IR に戻すより、型付きの中間IRを置くほうが Core の「型を持たない」という性質を保てる。もう一つは、ラムダ持ち上げ後の Core IR の上の特殊化が、閉包の確保を除けないことが測定で示されたときである。

追加する場合の設計は、Lean の LCNF に倣い、直接形のANF (束縛に型を付ける)、join point、明示的な具体化、単相化済みのインスタンスを持つ木にする。Lean 4.22 のリリースノートには、LCNF の `simp`、CSE、`floatLetIn` と、LCNF から IR への変換 (`toIR`) の修正が並んでいる ([Lean 4.22 のリリースノート](https://lean-lang.org/doc/reference/latest/releases/v4.22.0/))。ただし、これは Lean の事情であり、eml の規模で同じ層が要るという根拠にはならない。

### Q3 Core IRの再設計と、`simplify` と `perceus` の置き場

Core IR の核は維持する。次の三点を変える。

1. **`saved` を IR に持たせない。** `saved` は「呼び出しの後で生きている変数から結果を除いたもの」であり (`docs/spec/core-ir.md:326`)、生存解析から導ける。`docs/spec/core-ir.md:265` と `docs/implementation/architecture.md:207` は「パスの間で古くなる情報は IR に書かない」としているが、`saved` は Perceus が書き込み、verifier が R7 で照合する。連続したスタックで呼ぶVMでは、生きている変数はレジスタに残るので `saved` は要らない。CEK が使う分は、読み込み時に側の表として計算すればよい。
2. **verifier に「効果を下げた後」の段を足す。** 既に3つの段がある (`docs/spec/core-ir.md:330` の verifier)。4つ目は `perform` / `handle` / `resume` / `mask` を含まないこと、継続のクロージャと evidence の引数が位置の規則を満たすことを検査する。
3. **自己末尾呼び出しをループにする計画は、IR の変更より先にバイトコードの出力で実現する。** Beans の論文は、再帰的な末尾呼び出しを `goto` にする (本文を読んだ)。eml のバイトコードも、自己の末尾呼び出しを関数の入口へのジャンプにすれば、IR の規則 R2 を緩めずに済む。R2 を緩めるのは、ループ不変式の移動や借用のループ引数が要るときにする。

**Perceus は現在の Core IR に対して行うパスでよい。** 理由は、Core IR が一階で、ANF で、box の挿入によって引数の受け渡しが決まった後の形だからである。Lean の ExplicitBoxing も `inc` / `dec` をまだ扱わない段として書かれており、RC の挿入は boxing の後になる ([upstream のソース](https://github.com/leanprover/lean4/blob/6562e9dfa09e4e9b1839dbabbdca26dcea1b91f9/src/Lean/Compiler/LCNF/ExplicitBoxing.lean))。ただし upstream の現行版では boxing が LCNF の impure 段に置かれており、Beans 論文の λRC の時代の IR 版とは層の境が違う。注意は二つある。evidence passing は Perceus の前に置く (設計文書どおり。継続のクロージャと合流ブロックを新しく作るため)。Perceus より後のパスは RC の命令を壊さない変換に限る。

**`simplify` は、旧版を復活させるのではなく、新しいパスとして Core IR の上に置いてよい。** 条件は、translate と box の挿入の間に置くこと (`docs/spec/core-ir.md:292`)、関数単位の作業リストで動かし、枝の部分木を置き換えないこと (旧版が2乗の時間だった原因、`docs/implementation/architecture.md:222`)、パスの終わりに番号を振り直して R1 から R9 を保つこと (`docs/future/roadmap.md:317` 以降) である。box の挿入の前に置くと、`f$boxed` や `box` / `unbox` が現れる前の具体的な Repr で関数を比べられる。

### Q4 Core IRの下にLIRを追加すべきか

独立したIRとしては追加しない。根拠は次のとおり。

- Cranelift は、IR の中のブロック引数と機械向けの `VCode` の二層を自分で持つ ([Cranelift の設計の記事](https://cfallin.org/blog/2020/09/18/cranelift-isel-1/))。`VCode` への変換は線形な1回の走査で、命令を最終の順に作る。eml がブロック引数付きの Core IR を渡せば、機械向けの層は Cranelift が担う。eml 側に LIR を置いても、レジスタ割り当てと命令選択が重複すると考える (推測)。
- Lean は IR を C に出力し、同じ IR を解釈する実行系 (IR インタプリタ) も持つ。インタプリタは、ネイティブのコードがある関数を呼ぶときにそちらへ切り替える ([Lean 4 の論文](https://lean-lang.org/papers/lean4.pdf))。つまり、「最適化後の IR を実行系の入力にする」構成で実績がある。
- `con` や `unpack` の明示的なメモリ操作への展開、`release` の高速経路の展開は、出力の段で行える。

代わりに、Q3の2の「下げた後」の段を定義する。その段が、バイトコードと Cranelift の共通の入力になる。LIR を足す条件は、(a) Cranelift や LLVM を使わない自前のバックエンドを作るとき、(b) 配置のオフセット、`Int` の幅、フィールドの unbox のような目的依存の最適化を、VM とネイティブの両方で共有したいとき、のいずれかである。

### Q5 各IRの最適化パス

| 層 | 追加する | 追加しない、または保留 |
|---|---|---|
| HIR | なし | 最適化は型情報が要るので、型検査の後の段で行う |
| translate (型を読む段) | 単相化と特殊化 (鍵は関数と型引数)、多相な位置の `Int` の `box` を減らす | 既存の既知コンストラクタと case-of-case はそのまま |
| Core (box の挿入の前) | 既知の閉包への `apply` を `call` に、小さい関数と Prelude の合成子 (パイプ演算子、`>>`、`<<`、`<\|`) のインライン化、既知のコンストラクタの jump threading、引数が定数の関数の特殊化、純粋な extern の定数畳み込み、使われない `let` の削除 (既存) | 型に依存する最適化は置かない |
| Core (Perceus の後) | 借用パラメータ、`release` から `release_reuse` への書き換え、`dup` と `decref` の打ち消し、即値と分かる scrutinee の `decref` の削除 (`docs/future/roadmap.md:299`) | 借用と reuse の前提が整うまで、それ以外の変換は置かない |
| lowered Core | evidence passing (Perceus の前に置く)、末尾の `perform` の短絡 | |
| バイトコード | 比較と分岐の融合命令、レジスタの彩色による枠の縮小、ジャンプのジャンプの除去、`unbox` の消去、自己の末尾呼び出しのループ化 | 汎用の SSA 最適化は置かない |
| ネイティブ (Cranelift) | Cranelift の mid-end に任せる | 制御フローの最適化は Cranelift の mid-end に頼れない |

最後の行の根拠は、Cranelift の開発者が、mid-end は制御フローをほとんど触らないと述べていることである ([Wasmtime のインライナの記事](https://fitzgen.com/2025/11/19/inliner.html))。jump threading と既知コンストラクタの畳み込みは、eml が Core IR 側で行う必要がある。

### Q6 CEKを削除してVMへ移行すべきか

削除しない。理由は三つある。設計文書が CEK を差分テストの基準として残すと決めている。evidence passing の変換後の IR を CEK で実行して UI テストと照合する、という検証の仕組みが設計文書にある (`docs/future/evidence-passing.md:95` 以降)。CEK の継続の意味は `multi` の区間の複写を含み、これを別の実装と比べる基準が他に無い。

移行の条件は次のとおりにする。VM が全 UI テストの出力と一致する。差分テストの入力は UI テストに加え、生成したプログラムも足し、出力が CEK と一致する。`RunStats` の RC の増減回数が一致する。満たしたら、`eml run` の既定を VM にし、CEK は `--reference` のようなフラグか `dev` 用の feature に移す。

### Q7 VMの設計

lowered Core から、直接レジスタ型のバイトコードを作る。LIR は挟まない。Cranelift が示すように、命令列への変換は1回の走査で済み、編集できるデータ構造は要らない。Core IR 自体が最適化の層であり、バイトコードは実行専用の形にとどめる。

| 項目 | 設計 |
|---|---|
| レジスタ | 関数ごとの連続した枠。Core の変数 (`CoreFn.vars`) を、生存区間の彩色でスロットに割り当てる。生存解析は `crates/eml_core_ir/src/liveness.rs:12` が既にある |
| ブロック引数 | 並列な代入として `move` 列に展開する。`switch` はタグの表引きか、2分岐なら条件ジャンプにする |
| 算術と比較 | `Extern::IntAdd` などの行ごとに専用の命令にする。引数を `Vec` にしない。オーバーフローとゼロ除算は `MayFail` の行の属性から、エラー位置の表 (pc から `Loc`) を引く |
| それ以外の extern | 表の行の番号と、引数のレジスタ範囲を持つ `CallExtern` にする |
| RC | `Dup r`、`Decref r`、`Release r, tag, keep_mask` を命令として持つ。即値の `Decref` は何もしない (現在の `Runtime::decref` と同じ、`crates/eml_interp/src/runtime.rs` の `decref`)。不死のリテラルの `dup` と `decref` は、検査付きヒープでは釣り合いを保つ (`docs/spec/runtime.md:22` の不死の物体) |
| 呼び出し | 引数をレジスタの連続した範囲に置き、`Call f, base, n` で新しい枠を同じ連続スタックに作る。末尾呼び出しは枠を再利用する。生きている変数はレジスタに残るので、`saved` の写しは要らない |
| 効果 | evidence passing の後の Core IR を実行するので、`handle` / `perform` / `resume` の命令は持たない。継続はクロージャで、ヒープのフレームの連結リストは使わない |
| 値の表現 | まず現在の `Value` (16バイトの enum) を保つ。ヒープの世代番号の検査 (`crates/eml_runtime/src/heap.rs:537`) を保ち、`debug_heap` の機能を失わない。8バイトのタグ付きポインタは、確保の除去の後に測定してから決める |
| `box` / `unbox` | 命令にせず、レジスタの別名として扱う。Lean 4.22 のリリースノートも、余分な `unbox` はネイティブでは問題にならないがインタプリタでは影響する、と述べている ([リリースノート](https://lean-lang.org/doc/reference/latest/releases/v4.22.0/)) |
| 安全性 | バイトコードの verifier は作らない。lowered Core の verifier を通った IR だけを入力にする。デバッグビルドでは、出力した命令列のレジスタ番号と飛び先の範囲を検査する |

実装は2段階にするのがよい。第1段は、現在の `Machine` を置き換えるだけの VM で、`Runtime` (ヒープ、継続のフレーム、handler の連鎖) を使い回す。`resume` の番地に `pc` を入れる設計 (`crates/eml_interp/src/machine.rs:305`) がこれを想定している。この段の利得は、`Atom` の読み取りと `Block` / `Stmt` の二重の添字を除くことで、非末尾の呼び出しのヒープのフレームは残る。第2段は、evidence passing の後で呼び出しを連続スタックに移し、フレームの確保を除く。

### 優先順位つきの提案

提案の結果として目指す構成は次のとおり。効果を含む Core IR を CEK が実行し (参照実装)、evidence passing を通した lowered Core を VM とネイティブが実行する。

```
HIR + 型の側の表 → translate (単相化) → Core の最適化 ─┬→ boxing → contract → Perceus → CEK (参照実装)
                                                        └→ evidence passing → boxing → contract → Perceus (+ 借用・reuse)
                                                             = lowered Core ─┬→ バイトコード → VM
                                                                             └→ Cranelift → ネイティブ
```

優先度の高い順に並べる。成熟度は、確立した実践、査読済みで採用は少ない、プレプリントのみ、の3つで示す。

#### 提案1: CEKの確保を除き、ベンチマークの基盤を作る (最優先)

- 根拠: 上の測定。`crates/eml_interp/src/machine.rs:113`、同259行、同293行、`crates/eml_interp/src/runtime.rs:77`、同83行、同204行。
- 変更: `Rhs::Extern` の引数を固定長の配列にする。`Env` を呼び出し間で使い回す。`save` と `Frame::Return` の退避を `SmallVec` 相当にする。引数の `Vec` を除く。あわせて、`fib`、`loop`、`state`、`list`、`tree` のプログラムを `RunStats` と実行命令数の基準にする。
- 期待する効果: 測定では実行命令の約37%減、実行時間の27%から51%減。VMの利得を測る基準線が上がる。
- 費用とリスク: 小さい。テストの期待値は変わらない (`RunStats` の RC の回数も変えない)。
- 成熟度: 確立した実践。
- 検証: UI テストの全出力が一致すること、`RunStats` が同一であること、callgrind の実行命令数と実行時間の改善を、測定の環境と一緒に記録する。実際の差分は `experiments/interp-alloc-hack.diff` にある (そのままの採用は想定しない。`Env::reset` のあと `Env::new` が未使用になる)。

#### 提案2: `saved` を派生情報にし、verifier に「下げた後」の段を足す

- 根拠: `crates/eml_core_ir/src/lib.rs:357`、`docs/spec/core-ir.md:62`、同326行。
- 変更: `Rhs::Call` から `saved` を除く。verifier の R7 は、生存解析から導いた所有の集合と、その時点で所有している RC の対象を比べる。CEK は読み込み時に `saved` の表を作る。4つ目の段は、効果の命令を含まないことなどを確かめる。
- 期待する効果: Core IR が CEK のフレーム形式から独立する。VM、evidence passing、ネイティブが同じIRを共有できる。
- 費用とリスク: 中。IR のテキスト形式、`pretty`、`parse`、verifier、Perceus、インタプリタ、Core IR のダンプのスナップショットを変える (`docs/implementation/testing.md:14` の「テストの変更の運用」の分類では期待値の変更)。
- 成熟度: 確立した実践 (一般的な設計原則。Lean の IR の同様の欄の有無は未確認)。
- 検証: UI テストの出力が変わらないこと、`verify` が所有の誤りを検出する既存のテストが通ること。

#### 提案3: Core IR の上の最適化パス (translate と box の挿入の間)

- 根拠: `docs/spec/core-ir.md:292`、`docs/future/roadmap.md:317`、Cranelift の mid-end が制御フローを変えないこと ([Wasmtime のインライナの記事](https://fitzgen.com/2025/11/19/inliner.html))、Lean の `@[inline]` と `@[specialize]` ([論文](https://lean-lang.org/papers/lean4.pdf))。
- 変更: 既知の閉包への `apply` を `call` に置き換える。小さい関数をインライン化する。Prelude の `|>`、`>>`、`<|` を展開する (これらは普通の関数である。`std/Prelude.em:66`、同68行)。ブロック引数を通した既知のコンストラクタの jump threading (`docs/future/roadmap.md:319` の規則と R3 を守る作り方に従う)。引数が定数の関数の特殊化。純粋な extern の定数畳み込み。
- 期待する効果: クロージャの確保と間接呼び出しを減らす。Lean がインライン化と特殊化で抽象の負担を除いている点は、eml の Prelude の `>>` などにも当てはまると考える (推測)。
- 費用とリスク: 中。コードの肥大、デバッグの難しさ。番号の振り直しで R2 と R3 を保つ作業が要る。
- 成熟度: 確立した実践。
- 検証: `RunStats` の確保数、RC の増減回数、callgrind の命令数を、パスの有無で比べる。UI テストの出力が変わらないこと。パスの前後を CEK で実行して差分を取る。ベンチマークは高階関数を使うプログラム (map と fold のループ) を足す。ここでの結果が eml の負荷に移るには、実際の使い方が高階関数と Prelude の合成子を多用していることが要る。

#### 提案4: translate での単相化

- 根拠: 型変数の位置の `Int` の変換が302,149回と200,125回あったという記録 (`docs/future/roadmap.md:298`)、`range` の Core IR の `box`。S4 の Eq/Ord/Show の特殊化が translate で行われる計画 (`docs/superpowers/specs/2026-10-07-redesign-design.md:78`)。
- 変更: (関数、型引数) を鍵とするインスタンス表を持ち、必要なものだけを translate で生成する。多相再帰は、上限を超えたら一様な版にフォールバックする。
- 期待する効果: 多相な位置の `box`、`unbox`、`decref` の削減。`Eq` の特殊化の前提。
- 費用とリスク: 中。コードの肥大とコンパイル時間。`f$boxed` のスキームとの整合。
- 成熟度: 確立した実践。
- 検証: UI テストの `box` / `unbox` の静的な数と動的な回数、`peak_objects`、コンパイル時間。

#### 提案5: evidence passing を Core IR から Core IR への変換として作る

- 根拠: `docs/future/evidence-passing.md:20`、[Xie と Leijen, ICFP 2021](https://dl.acm.org/doi/10.1145/3473576)。現在の `perform` は handler の連鎖をたどり (`crates/eml_interp/src/effects.rs:11`)、非末尾の再帰ごとに別のエフェクトの handler を設ける形では費用が深さに比例する (`docs/implementation/status.md:58` の「深さと性能」)。
- 変更: 設計文書どおり、Core IR に row の情報を持たせ、translate と box の挿入の間に変換を置く。先に、CEK で変換後の IR を実行して差分テストをする。`never` の中断での `Lin` の値の解放を、yield の経路に命令として出すか、継続を組み立ててから解放するかを決める。
- 期待する効果: ネイティブとVMが継続の捕捉を持たずに済む。`perform` の費用が定数になる。
- 費用とリスク: 大。yield の確認が全関数に入る。`multi` の継続の再開の費用。row の情報の保守。
- 成熟度: 査読済みで、Koka の C バックエンドが使う (論文の主張)。eml への採用は未実施。
- 検証: 変換の有無で UI テストの出力が一致すること、`handler_visits` が深さに依らないこと、`multi_choice` などの継続を再開する多重度のテストが通ること。

#### 提案6: レジスタ型バイトコードVM (2段階)

- 根拠: Q7 の設計。[Shi ら](https://dl.acm.org/doi/10.1145/1328195.1328197) の評価は、Java のスタックコードをレジスタコードにした場合のものである。
- 変更: 第1段は `Machine` の置き換え (`Runtime` を再利用)、第2段は evidence passing の後で連続スタックに移行する。
- 期待する効果: 提案1の後でさらに、`Atom` の読み取りと二重の添字、`extern` の `call_extern` の分岐を除く。第1段の追加の利得は小さい可能性がある (推測)。提案1の後の `fib 22` の内訳では、`run` が30%、`transfer` が15% を占める。
- 費用とリスク: 中から大。バイトコードの出力、エラー位置の表、デバッグ用の逆アセンブル。
- 成熟度: VM の設計は確立した実践。eml への利得は未測定。
- 検証: 第1段のプロトタイプで、比較と分岐の融合命令のある版とない版の実行命令数を測る。CEK と全 UI テストの出力と `RunStats` を比べる。

#### 提案7: Perceus の拡張 (drop-guided reuse と借用パラメータ)

- 根拠: [Lorenzen と Leijen, ICFP 2022](https://dl.acm.org/doi/10.1145/3547634)、[Ullrich と de Moura, IFL 2019](https://arxiv.org/abs/1908.05647)、`docs/future/roadmap.md:302`。
- 変更: `release` を `release_reuse` に書き換え、対応する `con` を `con@token` にする。借用の推論は、末尾呼び出しの輪の上の仮引数を所有にする。reuse のトークンは `saved` に入れない。
- 期待する効果: リストと木の更新での確保の削減。Lean の論文では `-reuse` と `-borrow` を無効にすると、ベンチマークによって最大3.23倍と1.16倍遅くなった (本文)。この数値は Lean のワークロードのもので、eml の S4 のスクリプトが同じ構造を持つとは限らない。
- 費用とリスク: 中。verifier の所有の規則 (R6、R7) の拡張。`multi` の継続との相互作用。
- 成熟度: 査読済みで、Koka と Lean が実装している。
- 検証: `peak_objects` と確保数、`tree` と `list` のテスト、`multi` を含むテスト。

#### 提案8: ネイティブは lowered Core から Cranelift で出力する

- 根拠: [Cranelift IR](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md) のブロック引数。`docs/spec/core-ir.md:10` が、ブロックの列は Cranelift のブロック引数へ1対1で写ると書いている。標準ライブラリを一度だけ翻訳して使い回すための ABI の規則 (`f$boxed` と T3、`docs/spec/core-ir.md:156` 以降) が既にある。
- 変更: 提案2と提案5の後で、バックエンドの試作を作る。Lean のインタプリタが、ネイティブのコードがある関数ではそちらへ切り替えるのと同様の、標準ライブラリはネイティブ、ユーザーのコードはバイトコードという混在は、ABI が安定していれば可能なので、実験として検討する (推測、未検証)。
- 費用とリスク: 大。`Int` の幅、オブジェクトモデル、記述子 (`docs/future/roadmap.md:302` の処理系)。
- 成熟度: 確立した実践 (Cranelift)。混在の実行は Lean の例のみ。
- 検証: 同じ UI テストを3つの実行系 (CEK、VM、ネイティブ) で実行して出力を比べる。

#### 提案9 (投機的): copy-and-patch による JIT

- 根拠: [Xu と Kjolstad, OOPSLA 2021](https://arxiv.org/abs/2011.13127) の要旨。バイトコードから低コストで機械語を作れる。
- 位置づけ: 投機的。VM が安定し、起動時間が支配的になった後でのみ検討する。ステンシルの作成に LLVM とクロスプラットフォームの管理が要る。
- 検証: プロトタイプで、VM に対する速度と起動の遅延を測る。

### 現在の設計で維持してよい点

- 前向きの辺だけの基本ブロック列、ブロック引数、Repr、`box` / `unbox` の位置の規則は、Lean と Cranelift の設計と一致し、変えない。
- 末尾呼び出しの保証 (T3 と `contract`) は、Beans の論文が指摘する「RC の挿入が末尾呼び出しを壊す」問題を、`ret` を上げることで避けている。ABI をその関数と末尾の位置で呼ぶ関数だけで決める規則は、標準ライブラリの再利用の要件と整合する。
- HIR を構文に忠実な木に保ち、型を側の表に置く構成。インクリメンタル化と LSP の要件に合う。
- 型検査の `mask` の記録と、それを Core IR の呼び出しに付ける設計。エフェクトの健全性を実行時の意味に写す方法として、変える理由が無い。

## 確度と未確認事項

- コードに関する事実 (行番号、パスの構成、IR の形) は読んで確かめた。IR のダンプは、`Session::compile_until` と `pretty` を使うコピー上のサンプルで取った。
- 測定は1台のサンドボックスで、release ビルド、各3回の中央値である。揺れが大きく (たとえば `loop` は2.309から2.494 s)、絶対値を他の環境にそのまま使えない。実行命令数は callgrind の値で、決定的である。測定したプログラムは `fib`、単純なループ、`handle` を使う再帰の3つだけで、データ構造とクロージャを多用する負荷は測っていない。
- 2か所の修正は、効果を確かめるためだけのコピー上の変更である。マウントされたコードは変更していない。
- 引用の検証。本文の `path:行` の引用は、抽出して該当行の内容を確かめた。`docs/` への引用にも行番号を付けた。節の見出しの行を指すものは、その節全体を指す。
- 論文の読み方。Ullrich と de Moura の論文は全文を読んだ。Xie と Leijen、Retrofitting OCaml、Perceus、Lorenzen と Leijen、Maurer ら、Shi ら、Marlow と Peyton Jones、Maranget、copy-and-patch は、要旨と検索結果に含まれる本文の抜粋だけを読んだ。本文を読んでいない箇所の主張 (たとえば Perceus の drop specialization との対応) は「推測」と書いた。
- Lean の ExplicitBoxing と ExplicitRC は、公開されたドキュメントの抜粋を読んだ。Lean のソースの全体は読んでいない。Lean 4.22 のリリースノートは LCNF の存在を示す根拠として使い、設計の詳細は読んでいない。
- 未確認: 提案6の追加の利得 (第1段の VM が提案1の後にどれだけ速くなるか)、提案3と提案4の効果の大きさ、`Value` が16バイトであることとヒープの世代番号の検査の費用、`multi` の継続を evidence passing で再開する費用。これらはプロトタイプで測るべき項目である。
- 判断が分かれる点。Q2 (中間IRを足すか) は、設計の好みの差が出る。私は現在の証拠では不要と判断したが、Lean は LCNF を持つ。条件を明示したので、条件が満たされたら再検討できる。
- 一般的な評価の注意。論文のベンチマークの結果 (Koka、Lean、Java VM の数値) は、eml の負荷に当てはまるとは限らない。当てはまるには、eml のプログラムのデータ構造の更新の頻度と、効果の使い方が、論文の負荷と近いことが要る。

## 参考文献

### Web の資料

- eml の設計文書 `docs/` (リポジトリ内。`spec/core-ir.md`、`implementation/architecture.md`、`future/roadmap.md`、`future/evidence-passing.md`、`superpowers/specs/2026-10-07-redesign-design.md`)
- [Cranelift IR の文書 (Wasmtime リポジトリ)](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md)
- [Chris Fallin, "A New Backend for Cranelift, Part 1: Instruction Selection" (2020)](https://cfallin.org/blog/2020/09/18/cranelift-isel-1/)
- [Nick Fitzgerald, "A Function Inliner for Wasmtime and Cranelift" (2025)](https://fitzgen.com/2025/11/19/inliner.html)
- [Lean 4.22.0 リリースノート (2025-08-14)](https://lean-lang.org/doc/reference/latest/releases/v4.22.0/)
- [Lean 4 の論文 (PDF)](https://lean-lang.org/papers/lean4.pdf)
- [Lean `src/Lean/Compiler/LCNF/ExplicitBoxing.lean` (commit 6562e9d、全文を読んだ)](https://github.com/leanprover/lean4/blob/6562e9dfa09e4e9b1839dbabbdca26dcea1b91f9/src/Lean/Compiler/LCNF/ExplicitBoxing.lean)
- [lean4 issue #15346 (ExplicitBoxing の LCNF から IR への変換の位置を示す、読んだ)](https://github.com/leanprover/lean4/issues/15346)
- リポジトリ内の `Cargo.lock` と `flake.nix` (依存の版。「調査結果」の 0 を参照)

### 論文

- Alex Reinking, Ningning Xie, Leonardo de Moura, Daan Leijen. "Perceus: Garbage Free Reference Counting with Reuse". PLDI 2021. [DOI 10.1145/3453483.3454032](https://dl.acm.org/doi/10.1145/3453483.3454032) (要旨と抜粋のみ)
- Sebastian Ullrich, Leonardo de Moura. "Counting Immutable Beans: Reference Counting Optimized for Purely Functional Programming". IFL 2019 (arXiv 版は2020年改訂)。[arXiv:1908.05647](https://arxiv.org/abs/1908.05647) (全文を読んだ)
- Anton Lorenzen, Daan Leijen. "Reference Counting with Frame Limited Reuse". ICFP 2022. [DOI 10.1145/3547634](https://dl.acm.org/doi/10.1145/3547634) (要旨のみ)
- Luke Maurer, Paul Downen, Zena M. Ariola, Simon Peyton Jones. "Compiling without Continuations". PLDI 2017. [DOI 10.1145/3062341.3062380](https://dl.acm.org/doi/abs/10.1145/3062341.3062380) (要旨のみ)
- Ningning Xie, Daan Leijen. "Generalized Evidence Passing for Effect Handlers: Efficient Compilation of Effect Handlers to C". ICFP 2021 (PACMPL 5, ICFP, Article 71). [DOI 10.1145/3473576](https://dl.acm.org/doi/10.1145/3473576) (要旨と抜粋のみ)
- KC Sivaramakrishnan, Stephen Dolan, Leo White, Tom Kelly, Sadiq Jaffer, Anil Madhavapeddy. "Retrofitting Effect Handlers onto OCaml". PLDI 2021. [arXiv:2104.00250](https://arxiv.org/abs/2104.00250) (要旨と抜粋のみ)
- Simon Marlow, Simon Peyton Jones. "Making a Fast Curry: Push/Enter vs. Eval/Apply for Higher-Order Languages". JFP 16(4-5), 2006. [DOI 10.1017/S0956796806005995](https://www.cambridge.org/core/journals/journal-of-functional-programming/article/making-a-fast-curry-pushenter-vs-evalapply-for-higherorder-languages/02447DB613E94DC35ACDCB24DB39F085) (要旨のみ)
- Luc Maranget. "Compiling Pattern Matching to Good Decision Trees". ML Workshop 2008. [DOI 10.1145/1411304.1411311](https://dl.acm.org/doi/10.1145/1411304.1411311) (要旨と抜粋のみ)
- Yunhe Shi, Kevin Casey, M. Anton Ertl, David Gregg. "Virtual Machine Showdown: Stack Versus Registers". ACM TACO 4(4), 2008. [DOI 10.1145/1328195.1328197](https://dl.acm.org/doi/10.1145/1328195.1328197) (要旨と抜粋のみ)
- Haoran Xu, Fredrik Kjolstad. "Copy-and-Patch Compilation: A Fast Compilation Algorithm for High-Level Languages and Bytecode". OOPSLA 2021 (PACMPL 5). [arXiv:2011.13127](https://arxiv.org/abs/2011.13127) (要旨のみ)
- Anton Lorenzen, Daan Leijen, Wouter Swierstra. "FP²: Fully in-Place Functional Programming". ICFP 2023 (PACMPL 7, ICFP, Article 198). [DOI 10.1145/3607840](https://doi.org/10.1145/3607840) (要旨と本文の抜粋)
- Matthew Lutze, Philipp Schuster, Jonathan Immanuel Brachthäuser. "The Simple Essence of Monomorphization". OOPSLA 2025 (PACMPL 9, OOPSLA1, Article 116). [DOI 10.1145/3720472](https://doi.org/10.1145/3720472) (要旨と抜粋)
- Marcial Gaißert, CF Bolz-Tereick, Jonathan Immanuel Brachthäuser. "Tracing Just-in-Time Compilation for Effects and Handlers". OOPSLA 2025 (PACMPL 9, OOPSLA2, Article 307). [DOI 10.1145/3763085](https://doi.org/10.1145/3763085) (要旨と抜粋)
- Cong Ma, Jonghyun Jung, Yizhou Zhang. "Virtualizing Continuations". PACMPL 2026. [DOI 10.1145/3808289](https://doi.org/10.1145/3808289) (要旨のみ)
- Leonardo de Moura, Sebastian Ullrich. "The Lean 4 Theorem Prover and Programming Language". CADE 2021 (system description). [Springer](https://link.springer.com/chapter/10.1007/978-3-030-79876-5_37)、[PDF](https://lean-lang.org/papers/lean4.pdf) (抜粋のみ)

### 補足ファイル

- `experiments/fib.em`、`experiments/loop.em`、`experiments/state.em`: 測定に使ったプログラム
- `experiments/interp-alloc-hack.diff`: 2か所の変更の差分 (`eml_interp` のコピー上)
