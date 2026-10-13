# S9v `eml_rt` を Verus で検証した1つの crate にする

位置づけ: 作業の設計。S9a と S9b の間に入れる作業を定める。S9v を終えたら、決まったことを下の「文書」に挙げる文書に移し、この文書を削除する。

## 目的

S9a の `eml_rt` は、純粋な関数と `share` のたどりだけを、`crates/eml_rt/verus/` に置いた写しで Verus に証明させている。写しと本体が同じコードであることは、`verus_copies.rs` の文字列の照合で確かめる。`unsafe` の核 (確保器、ヘッダとフィールドの読み書き、RC、解放の連鎖、reuse と token、検査モード) は証明の外にあり、Miri で全体の単体テストを流して確かめている。Miri は strict provenance の設定で約 18 分、それに Tree Borrows を足した設定で約 23 分かかる。

S9v では、次の3つを行う。

- vstd への依存を受け入れ、本体のコードを `verus!` の中に書く。写しと照合をやめ、`cargo verus verify -p eml_rt` が crate 全体を検証する
- 証明の範囲を `unsafe` の核まで広げる。生のポインタの読み書きは、どれも Verus の権限 (`PointsTo`) の上で行う
- Verus で確かめられない部分を、名前を挙げた信頼の一覧 (下の「信頼の一覧」) にまとめる。Miri は、その部分を通るテストだけを流す。今後の FFI の境界もこの一覧に入る。今の `Foreign` の物体 (Rust の値と破棄処理を持つ物体) が FFI の前身なので、Miri のテストはそこを重点にする

基盤は、この設計の前にコミット 6c6aafa と 3614fe1 で入れた。nixpkgs は nixos-unstable を追い、Rust の toolchain は stable も nightly も fenix から取る。stable は Verus が求める 1.98.1 に揃える。Verus と verusfmt は、release の archive を flake の input にし、`nix/verus.nix` と `nix/verusfmt.nix` が devShell に入れる。

`eml_rt` はまだパイプラインから使わない。UI テストの出力と `bench/` の仕事の回数は変わらない。

完了の条件は次のとおりである。

- `cargo verus verify -p eml_rt` が 0 errors で通る。かかった時間を記録する
- 信頼の一覧のテスト (下の「信頼の一覧のテスト」) が通り、その「未証明」と「`verus!` の外」の一時の欄が空である
- `eml_rt` の単体テストが、速いモードと検査モードの両方で通る。既存のテストと UI テストは、下の「テストの変更」に挙げたもののほかは、変えずに通る
- Miri が、`trusted` の module のテストを、`-Zmiri-strict-provenance` の設定と、それに `-Zmiri-tree-borrows` を足した設定の両方で通す。かかった時間を記録する
- `bench/rt.sh` の速いモードの命令の数が、S9v を始める前に 1.98.1 で取り直した基準 (下の「計測」) から、どの場合も 0.5% を超えて増えない。超えたら原因を調べ、直せないものは理由を spec に足して相談する
- `cargo clippy --all-targets`、`cargo fmt --check`、`eml_rt` の `verusfmt --check` が通る
- `nix build` が通り、x86_64-linux の出力が評価できる。Linux の組み立ては、この環境に Linux の builder がないので確かめない

## 段の位置

S9v は S9a の後、S9b の前に置く。S9b の extern の語の ABI は、FFI の最初の境界になる。境界を信頼の一覧と Miri のテストに足す決まりを、S9b より前に決めておくためである。段の列の S9 の1行は、検証の列の「`unsafe` の核を Miri で検査する」を「核を Verus で検証し、信頼する部分を Miri で検査する」に改める。S9 の作業の表には S9v の行を足す。

## 依存と道具

- `eml_rt` は `vstd = "=0.0.0-2026-10-04-0306"` に依存する。Verus 0.2026.10.04.426d8b0 の cargo-verus の toolchain の表が、この版を指すためである。Verus の版を上げるときは、flake の2つの input、`flake.nix` の `stable` の版、vstd の版を揃えて変え、`cargo verus verify --check-toolchain` で確かめる
- `Cargo.lock` に vstd とその依存 (verus_builtin、verus_builtin_macros、verus_syn など) が入る。`eml_cli` は `eml_rt` に依存しないので `eml` バイナリには入らないが、`nix build` は lock の crate をすべて取る
- `eml_rt` の `Cargo.toml` に `[package.metadata.verus] verify = true` を置く。`verus_keep_ghost` と `verus_only` の cfg は `unexpected_cfgs` の `check-cfg` に足す
- `nix/verus.nix` は、`libvstd.rlib` と `vstd.vir` も残す。Verus の起動の部分が、部品がそろっているかを確かめるためである
- rustfmt は `verus!` の中を整えないので、`eml_rt` のファイルは verusfmt で整える。ほかの crate は今までどおり rustfmt で整える
- clippy は `verus!` の展開の後のコードも見る。消した ghost のコードから出る警告 (spike では `if_same_then_else` と `redundant_static_lifetimes`) は、`eml_rt` の crate の頭で、理由を書いて `allow` する。どの警告を許すかは、段 1 で決めて一覧にする
- rust-analyzer と verus-analyzer については何も求めない

## 証明すること

公開の API の前提 (下の「API の約束」と「検査モードの約束」) の下で示す。どれも、値の全域と任意の大きさのヒープについて成り立つ。

1. メモリの安全: 検証したコードの生のポインタの読み書きは、どれもヒープが持つ権限で行う。解放の後の使用、二重の解放、ブロックの外の読み書き、書いていない語の読み込みは起きない。借用の規則 (Stacked Borrows と Tree Borrows) は Verus の権限のモデルに入らないので、この証明の外にある (信頼の一覧の T8)
2. ヒープの形の不変条件: ページの一覧、級ごとの空きリスト、切り出しの位置、各ブロックの権限が食い違わない。ブロックの番地の下位 16 ビットを落とすと、そのブロックを含むページの頭になる。この不変条件は `Heap` の閉じた述語 `wf` にし、公開のどの操作も `requires` と `ensures` で保つ。`Heap` と `Pages` には Verus の型の不変条件を使わない。型の不変条件を持つ型の欄を `&mut` で触る関数は `no_unwind` でなければならず、`Vec::push` も vstd の確保も `no_unwind` でないためである (例外は `Meta` で、下の「権限の形」)
3. RC の正しさ: 各物体の rc は、抽象のモデル (下の「抽象のモデル」) の参照の数と対応する。物体を空けるのは、張り付いていない物体の参照がなくなったときだけで、そのときは必ず空ける。解放の連鎖は、子の参照を1つずつ手放す。この性質から、生きている物体の走査する語が指す物体はどれも生きている (1 に要る)。ただし T3 の破棄処理が unwind すると、連鎖の残りは空けずに漏れる (T7)
4. `share`: `share_walk` の今の証明 (`Ok` と `LinShared` の場合) を、ヒープの上の `Graph` の実装と結ぶ。さらに検査モードについて、`Dead` を返すのは届いた物体が生きていないときだけで、`LinReachedTwice` を返すのは同じ `lin` の物体に2回届いたときだけであることを示す
5. `release` と reuse: `release` と `release_reuse` の結果を、抽象のモデルの遷移で書く。部品を並べた展開 (`is_last` と `free_block`) の遷移を合わせたものが `release` の遷移に等しいことは、モデルの上の補題 (proof fn) で示す。token の箱を1回だけ使うことは、`con_at`、`reuse_in_place`、`drop_token` の前提にする
6. 検査モード: 下の「検査モードの約束」の性質
7. S9a の5つの性質 (ヘッダの詰め方、即値、RC の遷移、`share_walk`、`release` と reuse の判定) を引き継ぐ。ヘッダの権限が値を持つので、S9a で信頼していた「メモリから読んだ `Meta` が正しい形であること」は、証明する側に移る

## 権限の形

- ページは、vstd の `PointsToRaw` (切り出していない部分) と、ページの頭の `PointsTo<PageHeader>` に分けて持つ
- ヘッダは、rc の欄の `PointsTo<i32>` と、`meta` の欄の `PointsTo<u32>` の2つの権限にする。`&Header` は作らない。共有の物体の rc は、ほかのスレッドが atomic に書き換えるので、ヘッダ全体への参照は、rc を atomic で読み書きすることと両立しないためである。`meta` を書くのはほかのスレッドがその物体を読めないときだけなので、`meta` の欄だけへの参照は作ってよい
- 検査モードの前置きも、2つの欄の `PointsTo<u32>` にする
- ペイロードの語は、どれも `Word` の1語の権限 (`PointsTo<Word>`) にする。数の語、`BigInt` の値、生の語は、`without_provenance_mut` で作った `Word` に入れて書き、ビット列で読む。今の `cast::<u64>()` の読み書きと同じビット列になるので、メモリの中身は変わらない。空きリストの次のブロックはポインタなので、provenance を持つ `Word` として書き、ポインタとして読み戻す。ビット列から作り直すと provenance を失うためである。語の型を語ごとに変えずに済むので、使っている間のブロックで権限の型を変えることはない
- メモリを確保器に返すとき (大きい物体のページ、`Heap` と `Pages` を落とすとき) と、`Foreign` のペイロードを T3 に渡すときは、語の権限を `leak_contents` で書いていない状態にしてから `into_raw` で `PointsToRaw` に戻す。どちらも vstd の公理で、信頼の項目は増えない
- `Foreign` の物体のペイロードは、T3 の書き込みの関数に `PointsToRaw` で渡し、T3 が返す不透明な権限で持つ。破棄処理は、その権限を受けて `PointsToRaw` を返す
- `Heap` は、これらの権限を `Tracked` の欄に、ブロックの番地をキーにした写像で持つ。`share` のたどりの間は、権限の写像を `Graph` の実装に移し、たどりの後で戻す
- `Meta` だけは、Verus の型の不変条件 (正しい形であること) を持つ。`Meta` は `Copy` で `&mut` のメソッドを持たないので、型の不変条件の `no_unwind` の制約に当たらない。公開の `Meta::kind` が利用者の作った値を受けても、`Kind::from_bits` の `unreachable!` の枝に届かないことを示すためである。`Meta` を作るのは `Meta::new` と `with_lin` とメモリからの読み込みだけで、メモリの `meta` が正しい形であることはヒープの不変条件が持つ

## 抽象のモデル

ヒープの見方 (`Heap` の `view`) は、ブロックの番地からブロックの抽象の値への写像である。ブロックの値は次のものを持つ。

- 状態: 構築中、生きている、token の箱、空ける途中 (検査モードだけ)、空いた (検査モードだけが空いたブロックを残す)
- rc、`meta`、所有者 (速いモードでも ghost の値として持つ)、走査する語の列、生の語の列
- 語ごとの書き込みの状態: 書いていない、書いた、token の箱から残した。「残した」になるのは走査する語だけで、生の語は「書いていない」か「書いた」のどちらかである
- 検査モードの前置き (所有者 ID か不死の物体の数、状態、世代の番号)

参照の数は、ヒープの中の辺の数と、外の参照の数の和である。

- ヒープの中の辺は、生きている物体の走査する語のうち、「書いた」でその物体を指すものである。token の箱、空ける途中の物体、空いたブロックの語は辺に数えない
- 外の参照の数は、ヒープの外 (VM のレジスタや Rust の変数) が持つ、数えた参照の数で、ghost の値として `Heap` が持つ。外の呼び出し側は検証しないので、この数は API の約束の言葉としてだけ意味を持つ
- 不死の物体 (rc 0) は空けないので、参照の数を数えない。検査モードの不死の物体の数 (`acquire_immortal` で増やし、`decref` で減らす数) は、前置きの値として別に持つ。ダミーの物体はこの数も持たない (`UNCOUNTED`)
- 張り付いた数 (局所の `MAX` と、`DROP_LIMIT` より小さい共有の数) は、今の `rc.rs` の証明の理想のモデル (rc、本当の参照の数、張り付きの旗) をそのまま使う

操作は次のように数を動かす。

- `alloc`、`alloc_data`、`alloc_int`、`alloc_foreign` は、外の参照を1つ作る。`alloc_immortal` は数えない参照を返す
- `dup` は外の参照を1つ増やし、`decref` は1つ減らす
- `set_field` は、書く語の外の参照を1つ辺に移す。上書きされる語が「書いた」なら、その辺を外の参照に移す。「書いていない」と「残した」の語を上書きするときは、辺を動かさない
- `release` で残すフィールドは、辺から外の参照に移る

token の箱と、使い回した箱の語の状態は次のとおりである。

- `release_reuse` が返す token の箱では、残したフィールドの語は「残した」、残さないフィールドの語は「書いていない」になる。残さない子はもう空けたかもしれないので、その語を辺に戻してはならないためである
- `into_token` は子をもう処理した物体を受けるので、走査する語はどれも「残した」になる。呼ぶ側が子の参照を受け取ったか手放したかを、ヒープは知らないためである
- 生の語は、token の箱でも「書いた」のままにする
- `reuse_in_place` は同じ形の箱を返すので、語の状態をそのまま引き継ぐ
- `con_at` が使い回した箱では、前の物体の走査する語のうち、新しい `meta` の走査する語の位置にあるものだけが「残した」のまま残り、ほかの語 (前の生の語を含む) は「書いていない」になる

構築中の物体は、`alloc`、`alloc_immortal`、`con_at`、`reuse_in_place` が作る。今の約束 (「呼ぶ側は、ほかのヒープの操作の前に、`meta` に合うペイロードを書く」) をモデルに書いたものである。

- 構築中の物体には `set_field` と `set_raw` だけを使える。構築中の物体は一度に1つまでで、ほかのどの操作 (安全な関数を含む) も、構築中の物体が書き終えていることを前提にする。書き終えているとは、走査する語がどれも「書いた」か「残した」で、`BigInt` なら値の語が「書いた」であることである
- `alloc`、`alloc_immortal`、`con_at` の `meta` の `scan` は 255 でない。走査する語が 255 個以上の物体は `alloc_data` で作る。`scan` が 255 の物体は、走査する語の数をペイロードの先頭の数の語から読むので、数の語を書く前には、走査する語と生の語の境目が決まらないためである
- 書き終えた構築中の物体は、モデルの見方では生きている物体として数える。「残した」の語はそれぞれ辺に数え、その分を外の参照の数から引く。見方の spec fn がこの数え方をするので、`&self` の操作もモデルの状態を変えずに済む。呼ぶ側は、残した語の参照を持っていなければならない (下の「検査モードの約束」のモデルの前提)。token の箱を使い回す一意の道が、同じ位置の同じフィールドを書かずに済ませる特殊化は、この形で書ける

読むだけの操作 (`field`、`raw`、`is_unique` など) の前提は、語がモデルで生きている物体 (参照の数が正か、不死) を指すことにする。数えた参照を持つ場合と、Perceus の借りたフィールドと基本操作の借用の引数の場合が、これに当たる。`raw` は、「書いた」の生の語だけを読む。

共有の物体も、今はこの `Heap` の見方に入れる。ほかの worker が並行に数えることは、まだ検証の対象にしない (信頼の一覧の T0)。VM の並列化のときに、Verus の並行の証明に置き換える。

## API の約束

公開の操作の形 (`unsafe fn`、引数、`Result`) は変えない。[オブジェクトモデル](../../spec/object-model.md) の「核の操作」の表と各操作の `# Safety` の文をもとに、条件を抽象のモデルの言葉で `requires` に書く。文と `requires` が同じ条件を指すように、文の側も必要なら書き直す。`requires` に現れるモデルの述語は `pub closed spec fn` にする。名前は crate の外に見えるが、中身と実行時の費用はない。

S9a では文書にだけあった前提も `requires` に書く。例えば `raw` は、読む生の語を先に書いてあることを前提にする。`Word::imm_value` は、語が即値であることを前提にし、今の `debug_assert` は外す。

内部の関数は、権限を引数で受け渡すか、`Heap` の ghost の欄から取り出して戻す。単体テストは、今と同じように公開の API と内部の関数を呼ぶ。ghost の引数を取る内部の関数は、テストでは `Tracked::assume_new()` を渡すか、公開の API を通して呼ぶ。

公開の API で変わるのは次の3つである。

- `Fast` と `Checked` が、空の enum から中身のない struct になる。Verus が空の enum を受け付けないためである
- `Heap` を `Sync` にしない印が、`PhantomData<(M, Cell<()>)>` から `PhantomData<(M, *mut ())>` になる。Verus が `Cell` を受け付けないためである。`Send` は今と同じく `unsafe impl` で足す。`Heap` は `RefUnwindSafe` になる
- モデルの述語の `pub closed spec fn` が増える

## 検査モードの約束

検査モードは、空けたブロックを使い回さず、権限を `Heap` に持ったままにする。大きい物体のページも返さない。そのため、検査モードの安全には、モデルの数が正しいことが要らない。

ただし今の検査モードには、同じ物体を二度空ける道がある。rc が 1 の物体の最後の参照を手放すと決めても、空けるのは作業リストから取り出したときなので、同じ子を2回指す親 (呼ぶ側が `dup` せずに同じ語を2つのフィールドに書いた親) を空けると、その子を2回積み、2回空ける。`Foreign` の物体なら破棄処理が2回呼ばれ、`Arc` のような値では解放の後の使用になる。S9v では、最後の参照と決めた時点で、その物体の前置きを「空ける途中」にする。`release_ref`、`release` の最後の参照の枝、`release_reuse` の子の処理の前がこれに当たる。2回目に届いたときは、生きていないので `UseAfterFree` を返す。`Err` で解放の連鎖が途中で止まったときは、積んだまま空けていない物体が「空ける途中」で残る。`Heap` を落とすときは、その中の `Foreign` の破棄処理を1回だけ呼ぶ。生存の列挙とリークは、「空ける途中」の物体を生きている物体と同じに扱う。`safe_wf` は、どのブロックも「空いた」になるのは高々1回であることを持ち、そこから隔離したバイト数が確保したバイト数を超えないことと、生きている箱の数が負にならないことを示す。

前提を3種類に分ける。

- 安全の前提: どちらのモードでも前提にし、検査モードも確かめない。破ると、検査モードでもメモリの安全が崩れうる
- モデルの前提: どちらのモードでも前提にし、検査モードも確かめない。破るとモデルの数が合わなくなるが、検査モードのメモリの安全は崩れない (速いモードでは、モデルの数が合わないことがメモリの安全を崩す)
- 検査する前提: 速いモードでは前提にし、検査モードでは確かめて `Err` を返す

`wf` も2つに分ける。`safe_wf` は権限の持ち方の不変条件で、どの操作の後も成り立つ。`model_wf` は抽象のモデルとの対応で、ghost の旗 `consistent` が真の間だけ成り立つ。性質は次のとおりである。

- どちらのモードでも、安全の前提の下で `safe_wf` を保つ
- `consistent` が真で、モデルの前提と検査する前提がすべて成り立つなら、`Ok` を返し、結果は速いモードと同じモデルの遷移になる
- `Err` を返すのは、その誤りに対応する違反が操作の前にあったときだけである
- 状態を変える前に `Err` を返す操作では、ヒープは変わらず、`consistent` も保つ。今のテストは、この形の `Err` の後もヒープを使い続ける。状態を変えた後で `Err` を返しうるのは、解放の連鎖 (`decref`、`release`、`release_reuse` の子の処理)、`release` の最後でない枝の `dup` の列、`share`、`con_at` (token の項目を取り出した後の `WrongClass`) である。その後は `consistent` が偽になってよく、オブジェクトモデルの約束どおりヒープを使わない。ただし `Heap` を落とすことは安全である
- token の表の項目は、`into_token` と `release_reuse` が添字を使い回さずに積み、取り出すと空にする。そのため、項目があればその箱は token の状態で、項目の世代の番号に等しい。この不変条件を証明するので、世代の番号が回ることは検出を外さない
- 不死の物体の数が `u32` を超えるときは panic する (T5)

各操作の前提は、次のように分ける。この表が、オブジェクトモデルの「核の操作」の表に代わる `requires` の一覧の骨組みになる。

| 操作 | 安全の前提 | モデルの前提 | 検査する前提 (検査モードの `Err`) |
|---|---|---|---|
| どの操作も | 語は即値か、このヒープが確保したブロックを指す。構築中の物体は書き終えている (`set_field` と `set_raw` を除く) | 読むだけの操作の語は、モデルで生きている物体を指す | 物体は生きている (`UseAfterFree`)。rc が正の物体を書き換えるか rc を読んで判定するなら、持ち主は今の worker である (`WrongOwner`) |
| 物体を受ける操作 (`meta`、`field`、`set_field`、`raw`、`set_raw`、`scanned_len`、`scanned`、`free_block`、`release`、`release_reuse`、`into_token`、`foreign`) | 語はポインタである (即値でも `Word::EMPTY` でもない) | | |
| `alloc` / `alloc_immortal` / `con_at` | `meta` の種類は `Foreign` でも `Free` でもない。`scan` は 255 でなく、走査する語はペイロードに収まる | | |
| `field` / `set_field` | `set_field` の物体は、ほかのスレッドが読めない (局所か、構築中) | `set_field` が書く語は、呼ぶ側が数えた参照を持つ | i 語目は走査する語である (`OutOfBounds`) |
| `scanned` | | | i は `scanned_len` より小さい (`OutOfBounds`) |
| `raw` / `set_raw` | `raw` で読む語は先に書いてある。`set_raw` の物体は、ほかのスレッドが読めない | | 物体はデータである (`WrongKind`)。i 語目は、走査する語の後のブロックの中の語である (`OutOfBounds`) |
| `unbox_int` | | | 物体は `BigInt` である (`WrongKind`) |
| `dup` | | 呼ぶ側が参照を持つか、借りている | 物体は `lin` ビットを持たない (`LinDup`) |
| `decref` / `release` / `release_reuse` | | 呼ぶ側が参照を1つ持つ。残さない子は呼ぶ側の参照と数える | 不死の物体の参照は `acquire_immortal` か `dup` で作った (`ImmortalOverRelease`)。共有の物体は `lin` ビットを持たない (`LinShared`)。`release` と `release_reuse` の物体はデータで、tag と走査する語の数が名指したものである (`WrongLayout`) |
| `acquire_immortal` | | | 物体は不死である (`NotImmortal`) |
| `free_block` | | 呼ぶ側が最後の参照を持ち、子の参照はもう処理してある | rc が 1 か -1 である (`NotLast`)。破棄処理を持たない (`FinalizerSkipped`) |
| `into_token` | | 呼ぶ側が唯一の参照を持ち、子の参照はもう処理してある | token にできる種類である (`NotTokenable`)。rc が 1 である (`NotUnique`) |
| `con_at` / `reuse_in_place` / `drop_token` | | token の箱の「残した」語の参照を呼ぶ側が持つ (`con_at` と `reuse_in_place` で、書き直さない語) | token はまだ使っていない (`StaleToken`。token の語が即値でなければ、`imm_value` を呼ぶ前に `StaleToken` を返す)。`con_at` の箱は級に合う (`WrongClass`)。`reuse_in_place` の `meta` は箱と同じ形で (`WrongMeta`)、箱の `lin` ビットを落とさない (`LinBitKept`) |
| `share` | | | 届く物体は生きている (`UseAfterFree`)。局所の `lin` の物体には1回だけ届き (`LinReachedTwice`)、共有の側の `lin` の物体には届かない (`LinShared`) |

速いモードも返す `Err` は今と同じ2つである。`foreign` の型の照らし合わせ (`WrongKind`) と、`reuse_in_place` が空の token を受けたとき (`StaleToken`) である。どちらも前提ではなく、どの入力でも決まった結果を返すことを証明する。

検査モードの `field`、`set_field`、`raw`、`set_raw` の範囲の検査は、今より強くなる。今はブロックのペイロードの外の添字だけを断るが、走査する語と生の語の取り違えと、`Foreign` の物体の生の語 (破棄処理の表を置く語) への書き込みも断る。どれも、断らなければ検査モードの安全が崩れる書き込みである (走査する語に生の値を書くと、解放の連鎖がその値をポインタとして読み、`Foreign` の表を上書きすると、破棄処理が壊れた関数のポインタを呼ぶ)。速いモードの動作は変わらない。

## 信頼の一覧

Verus で確かめない部分を、名前を挙げて次の表にまとめる。`external_body`、`assume_specification` などの信頼の項目は、どれもこの表のどれかに属する。

| # | 中身 | 理由 | Miri で確かめること |
|---|---|---|---|
| T0 | この `Heap` の物体に触れるのは1つのスレッドだけである。共有の物体の rc もこの `Heap` の操作だけが動かすとして、証明は逐次の実行を扱う | 並行の手順の証明は VM の並列化のときに行う | T2 の2つのスレッドのテスト |
| T1 | ページの確保と返却の関数。中身は `std::alloc::{alloc, dealloc}` で、確保に失敗したら `handle_alloc_error` を呼ぶ。約束は vstd の `allocate` と `deallocate` と同じ形にする | OS と確保器の境界。vstd の `allocate` は失敗すると `process::abort` を呼ぶので、使わずに自前で包み、今の動作を保つ | ページの確保と返却、64 KiB の揃え、大きい物体のページ、`Heap` を落とすときの返却 |
| T2 | rc と前置きの atomic の読み書き。欄の権限を受け、`AtomicI32::from_ptr` と `AtomicU32::from_ptr` で Relaxed と AcqRel の操作をする。約束は、T0 の下での逐次の値である | vstd の atomic は SeqCst だけを扱う。核では SeqCst を使わない | 局所と共有の `dup` と `decref`、`is_last` の2回の読み込み、検査モードの前置きの書き換え。2つのスレッドが同じ共有の rc の欄を並行に増減し、最後の参照を見るのがちょうど1つのスレッドであること (データ競合と順序の検査。別の worker のヒープで箱を空けることはまだ決めていないので、欄だけを使う) |
| T3 | `Foreign` の物体の中身。破棄処理の表 (関数のポインタ。`Finalizer` と `VTable` は不透明な外の型にする)、`TypeId` の照らし合わせ、`drop_in_place` | Verus は関数のポインタと `TypeId` を扱わない | 型の違う照らし合わせ、`Drop` を持つ値 (`Box`、`String`、`Vec`)、大きさ 0 の型、破棄処理が panic する場合、解放と `Heap` の破棄での破棄処理の呼び出しが1回だけであること |
| T4 | `verus!` の外の `Drop` の実装 (`Heap`、`Pages`) | Verus は利用者の `Drop` を扱わない。中身は検証した関数を呼ぶだけにする | T1 と T3 のテストが通る道 |
| T5 | 証明に使う std の約束のうち vstd にないもの。今わかっているのは `ptr::without_provenance_mut` (`Word::imm` が const fn のまま即値を作るため。`with_addr` は const fn でない) と、panic で止める関数 (大きさの桁あふれ、即値に収まらない `Word::imm`、不死の物体の数の桁あふれ、ページの配置の誤り) | vstd の std の約束が足りない。panic の関数は値を返さないことだけを約束する | なし (値の約束で、メモリに触れない) |
| T6 | `global` の宣言: 64 ビットの環境 (`size_of usize == 8`)、`Word`、`Header`、`PageHeader`、`Prefix` の配置 | Verus は `global` の宣言をコンパイル時に照らす。本体の `const` の assert もほかの環境のビルドを止める | なし |
| T7 | unwind。検証した関数は、決まった所でしか unwind しないとして証明する。決まった所は、T5 の panic の関数 (状態を変える前に呼ぶ。`new_page` の語の数の検査は、ページを確保する前に移す)、T3 の破棄処理 (箱を先に死んだことにしてから呼ぶ)、`Vec` の伸長 (確保の失敗は abort し、容量の桁あふれはアドレス空間より大きい量を要るので起きない) である | Verus の `ensures` は正常に戻ったときだけを扱う。unwind の後も `Heap` を落とすことは安全でなければならない | 解放の連鎖の途中で破棄処理が panic した後に、`Heap` を落とすこと |
| T8 | 借用の規則 (Stacked Borrows と Tree Borrows) | vstd の権限のモデルは、参照と生のポインタの借用の規則を表さない | 核の各操作を両方のモードで1回ずつ通す小さいテスト |
| T9 | Verus、Z3、rustc、vstd (その std の約束を含む。`Vec`、`mem::swap` など) | 道具そのもの | なし |

S9a で信頼していた `<[T]>::contains` の約束と `obeys_concrete_eq::<G::Node>()` は、なくす。`share_walk` の `visited_lin` の探索を、番地を比べる自前のループに書き換えるためである。vstd のポインタの `==` は provenance を見ないので、ポインタの型に `obeys_concrete_eq` は示せない。

FFI の境界 (S9b の extern の語の ABI と、その後の外の関数の呼び出し) を足すときは、この表に行を足し、Miri のテストを `trusted` の module に足す。

## 信頼の一覧のテスト

単体テストで信頼の一覧を固定する。S9a の `the_copies_trust_nothing_but_the_contains_spec` の役を引き継ぐ。

- 調べるのは、`eml_rt` の `src/` のテストでないコードである。`#[cfg(test)]` の module と `tests/` のディレクトリは除く。`examples/` は調べない
- 次の語を探し、見つけた項目 (語と、それを含む関数か型の名前) が、テストの中の一覧と同じであることを確かめる: `external_body`、`external_fn_specification`、`external_type_specification`、`external_trait_specification`、`verifier::external`、`assume_specification`、`exec_allows_no_decreases_clause`、`uninterp`、`axiom`、`global`、`assume(`、`admit(`、`Tracked::assume_new`、`Ghost::assume_new`。一覧の各項目には、上の表の番号を書く
- `verus!` の外のコードに `unsafe` がないことを確かめる。除くのは `unsafe impl Send` と `unsafe impl Sync` (`Word` と `Heap`) と、T4 の `Drop` だけである
- 一覧には一時の欄を2つ持つ。「未証明」は、まだ証明していないので `external_body` にした関数で、「`verus!` の外」は、まだ `verus!` に入れていないファイルである。段ごとに減らし、S9v の終わりに空にする
- 探し方の規則 (コメントと文字列を除くこと、`#[cfg(test)]` の module の見分け方) は、同じファイルの小さいテストで確かめる

`verus!` の外に置くのは、`fmt` の実装、T4 の `Drop`、`unsafe impl Send` と `Sync`、`Sync` でないことの型の assert、`leaks` の `BTreeMap` を組む安全な関数、テストだけにする。

## Verus に合わせるための書き換え

動作を変えずに、次のように書き換える。

- 内部では `NonNull<Header>` の代わりに `*mut Header` を使う。vstd は `NonNull` に約束を持たないためである。ポインタの計算は、`addr`、`with_addr`、`as` の型の変換で書く (`add`、`sub`、`cast`、`is_null`、`map_addr` は vstd に約束がない)。strict provenance は保つ
- rc と前置きの欄は、`AtomicI32` と `AtomicU32` でなく、`i32` と `u32` にする。atomic の操作は T2 の関数が `from_ptr` を通して行う。配置は変わらない
- 空の enum (`Fast`、`Checked`) は中身のない struct にし、`Cell` の印は `*mut ()` にする (上の「API の約束」)
- `pub const` の `Word::UNIT`、`Word::EMPTY`、`Token::EMPTY` は、`pub exec const` に `ensures` を付けて書く。private な欄の構成子を pub の const で使うには、この形が要る
- `expect`、`assert!`、`unreachable!` は、前提から証明するか、T5 の panic の関数を呼ぶ形にする。`Kind::from_bits` の `unreachable!` の枝は、メモリから読む `meta` が正しい形であることを証明するので、届かないことを示す
- `mem::take` は `mem::swap` にする。`while let` は `loop` と `match` にし、`decreases` を付ける
- 閉包を受ける `for_each_block` は、ブロックのヘッダの位置を `Vec` で返す関数にする。使うのは生存の列挙、不変条件の検査、リーク、`Heap` を落とすときで、どれも速い道でない
- `div_ceil` と `next_multiple_of` (`HEADER_WORDS`、`PREFIX_WORDS`、`FIRST_BLOCK`、`alloc_foreign` の語の数) は、足し算と割り算で書く
- 級の表 `CLASS_OF` は、`const` の中の while のループで作る今の形を保ち、表の中身が `class_of` の仕様に合うことを証明する。Verus がこの形の `const` を扱えなければ、表を値で書き、中身を `by (compute)` で確かめる
- `Stats` の数は `wrapping_add` で数える。2^64 回の確保は起きないので、桁あふれで panic させずに済ませる。`Pages` のバイト数と隔離したバイト数は、確保したページの大きさの和なので、アドレス空間に収まることを証明する
- `leaks` は、検証した関数が種類ごとの数を返し、`verus!` の外の安全な関数が `BTreeMap` に組む
- ページの頭の計算 (`addr & !(PAGE_BYTES - 1)`) は、bit_vector の補題で、ブロックを含むページの頭に等しいことを示す

下書きの段階 (spike) で確かめたのは、次のものである。

- `cargo verus verify` が devShell から動き、vstd に依存する crate を検証できる。同じ crate が `cargo test` と strict provenance の Miri で通る
- `M::CHECKED` (trait の関連の定数) を実行のコードと `requires` の中で使えること
- rc の欄を `i32` にし、`from_ptr` の関数で読み書きすること。`Tracked` の写像から権限を取り出して戻すこと
- ページの頭の補題、vstd の `allocate` と `PointsToRaw::split` と `into_typed`、`Vec` の上のループ
- 中身のない struct の `Mode`、`PhantomData<(M, *mut ())>`、`mem::swap`、panic の関数、`unsafe fn` の `requires`、`pub exec const` と private な欄、`without_provenance_mut` の約束

ほかの書き換え (`for_each_block` の置き換え、`CLASS_OF`、`Graph` の実装、`alloc_foreign` の中の `const { assert! }`、sealed な supertrait) は、段 1 から段 7 の中で確かめる。Verus は検証で `const fn` の制約を見ないので、検証が通っても `cargo build` が通るとは限らない。どの段でも両方を流す。

## Miri

Miri は、`crates/eml_rt/src/heap/tests/trusted.rs` (module `trusted`) のテストだけを流す。上の表の「Miri で確かめること」を1つずつ確かめる。中身は信頼の部分だけでなく、そこへ届く検証したコードも通るので、T8 の借用の規則もここで確かめる。

```sh
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_rt trusted::
```

全体の単体テストを Miri で流すことは、決まった手順から外す。テストの運用の変更で、[テスト戦略](../../implementation/testing.md) の「Miri」を書き換える。単体テストは `cargo test` で流し、メモリの安全は Verus の証明が受け持つ。`trusted` のテストは、どの設定でも数分で終わる大きさにする。全体を Miri で流したテストの大きさを `cfg!(miri)` で小さくする所 (`LONG` など) は、`trusted` のテストが使わなければ外す。

## 計測

段 1 の前に、S9v を始める前のコード (コミット 3614fe1。`eml_rt` は S9a の終わりの 30c04ae と同じ) を fenix の 1.98.1 で組み、`bench/rt.sh` の基準を取り直す。S9a の記録は rustc 1.95.0 で取ったもので、そのままでは compiler の違いと書き換えの違いが混ざるためである。macOS の記録だけを取る。S9v の終わりに同じ手順で測り、[計測の基準](../../implementation/benchmarks.md) に両方を記録する。

## 証明の進め方

下から順に、段ごとにコミットする。どの段の終わりでも、`cargo test`、`cargo verus verify -p eml_rt`、clippy、`cargo fmt --check`、`verusfmt --check` が通る。`trusted` のテストができた段からは、Miri も流す。まだ証明していない関数は `external_body` にして信頼の一覧のテストの「未証明」の欄に載せ、まだ `verus!` に入れていないファイルは「`verus!` の外」の欄に載せる。

0. 基準の取り直し (上の「計測」)
1. crate の準備: vstd の依存と道具 (上の「依存と道具」)。写しの証明 (`meta`、`word` の整数の関数、`rc`、`share_walk`、`check` の判定) を本体に移す。`crates/eml_rt/verus/` と `verus_copies.rs` を消す。信頼の一覧のテストを置く。ほかのファイルは「`verus!` の外」の欄に載せる
2. 語、ヘッダとペイロードの権限、T2 の atomic の関数、T1 の確保の関数、ページの確保器
3. `Heap` の `wf` と抽象のモデル、確保と構築中の物体、`dup`、`decref`、解放の連鎖、フィールドと生の語の読み書き、`is_unique`、`is_last`、`free_block`、`unbox_int`
4. `release`、`release_reuse`、token と `con_at`、`reuse_in_place`、`drop_token`、`release` の展開の補題
5. ヒープの上の `share`
6. 検査モード: 前置き、隔離、token の表、各検査 (強める範囲の検査を含む)、生存の列挙、不変条件の検査、リーク
7. `Foreign` (T3)、`Drop` (T4)、`trusted` の Miri のテスト、計測、文書

段の中の細かな順序は、計画で決める。

## テストの変更

- 成否の変更 (テストの削除): `crates/eml_rt/src/verus_copies.rs` の7つのテストを消す。写しがなくなり、照合するものがなくなるためである
  - `the_verus_copies_match_the_code`
  - `verus_only_syntax_is_removed`
  - `trait_specs_and_spec_fns_are_removed`
  - `items_in_an_impl_are_found_by_type_and_name`
  - `a_changed_body_is_reported`
  - `calls_are_found_with_or_without_a_space`
  - `the_copies_trust_nothing_but_the_contains_spec` (役は信頼の一覧のテストが引き継ぐ)
- 期待値の変更: なし。検査モードの範囲の検査を強めても、今のテストはどれも走査する語と生の語を取り違えず、`Foreign` の生の語にも書かないので、期待値は変わらない
- 機械的な追随: 内部の関数の形が変わる単体テストは、呼び方だけを直す。`assert` の値は変えない。`alloc.rs` の確保器のテスト (`for_each_block` を使う数え方は、ブロックの一覧の長さに変わる)、`rc.rs` の atomic の関数のテスト、内部の `meta::rc`、`meta::meta`、`meta::set_meta`、`check::prefix` を使う `heap/tests/share.rs` のテストがこれに当たる。`heap/tests/share.rs` の、`alloc_immortal` と `set_field` の間で `alloc_data` を呼ぶテストは、構築中の物体の約束を守るように呼ぶ順を入れ替える。`examples/rt_bench.rs` は公開の API だけを使うので、`Fast` の形が変わっても直す所はない
- テストの運用の変更: Miri は全体の単体テストでなく `trusted` のテストだけを流す (上の「Miri」)
- 足すテスト: 信頼の一覧のテスト、`trusted` の module のテスト、検査モードの強めた範囲の検査のテスト (誤りごとに1つ)、同じ子を2回指す親を空けると2回目が `UseAfterFree` になり破棄処理が1回だけ呼ばれるテスト

## 文書

S9v の終わりに、次の所を書き換える。

- `docs/spec/object-model.md`: 「依存を持たない crate」の記述 (vstd に依存する)、「語」の `map_addr`、ヘッダの図の `rc: AtomicI32` と前置きの「2つの `AtomicU32`」(欄は整数で、atomic の操作は T2 が行う)、「遷移の関数」の「`AtomicI32` への参照」、「`share`」の「ヒープの物体の実装は証明しない」、「核の操作」の表 (上の「検査モードの約束」の表に合わせ、範囲の検査を強めたこと、`alloc` と `con_at` の `scan` が 255 でないこと、モデルの前提を書く)、「前置き」と「報告する誤り」(空ける途中の状態と、同じ子に2回届いたときの `UseAfterFree`)、「`unsafe` の核」(核を Verus で検証すること、「std のものを直接使い、差し替えの層を置かない」を T1 と T2 の包みに改めること、信頼の一覧、FFI の境界を足すときの決まり)
- `docs/implementation/testing.md`: 「`eml_rt` の検証」の単体テストの `cfg!(miri)` と `is_last` の記述、「Miri」の節 (`trusted` のテストだけを流すこと、テストの数と時間)、「Verus」の節の全体 (写しの節を、本体の検証、信頼の一覧のテスト、verusfmt に書き換える)、「よく使うコマンド」
- `docs/future/roadmap.md`: 段の列の S9 の行の検証の列、S9 の作業の表 (2つの作業から3つに、S9v の行)、S9a で閉じた論点の loom の項 (写しを前提にした理由を改める)、「決めたこと」の「`unsafe` は小さい核に閉じ、Miri で検査する」
- `docs/future/multicore.md`: Verus と Miri の受け持ちの記述
- `docs/implementation/architecture.md`: crate の一覧と toolchain の項 (vstd の依存と verusfmt)
- `docs/implementation/benchmarks.md`: 取り直した基準と S9v の記録
- `CLAUDE.md`: `check.sh` を `cargo verus verify -p eml_rt` に、Miri の手順を `trusted::` に、verusfmt を足す
- コードのコメント: `rc.rs`、`word.rs`、`heap/share.rs`、`heap/check.rs` の、写しを前提にしたコメント
