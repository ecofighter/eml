# IR と呼び出し規約と線形性の包摂の見直し

- 作成: 2026-10-21
- 状態: 計画は確定。関門 G (T-6) の数値で、利用者が続けるか戻すかを決める

## 1. 目的と完了の条件

- 目的: 4つの論点 (変換パスと IR、translate で構造体を開くか、構造体と配列の C との互換、呼び出し規約と `once` への `Unr` の受け渡し) を調べた結論を実装する。変えるのは次の2つである。
  - 関数の値の呼び出し規約を、実行時の変換を持たない静的な規約に移す (D-3)
  - `Unr` の関数を `once` の位置に渡せるようにする (D-7)
  - ほかの論点は「変えない」が結論で、理由と見直す条件を docs に書く (D-1、D-2、D-4〜D-6)
- 完了の条件:
  - 6章の各タスクの完了の条件をすべて満たす
  - `cargo test`、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check` が通る
  - 関門 G の数値を利用者が承認している。承認されなければ、T-7 以降は行わず、T-5 を戻す (D-14)
  - docs/implementation/benchmarks.md の「今の数値」の3つの表を、きれいな作業ツリーで測り直している

## 2. 範囲

- 範囲に入るもの: eml_types の矢印の線形性の規則、eml_lir の `Mark` の比べ方、関数の値の ABI (eml_core_ir の `Role`、eml_lir の boxing・flatten・contract・verifier、eml_vm の `apply` と変換の仕組み)、対応する docs/spec と docs/future の記述
- 範囲に入らないもの:
  - 構造体のフィールドを親の物体に並べること、スカラーの要素を生で持つ配列、`Bytes`、ユーザーの FFI の実装 (D-5、D-6。roadmap に条件を書くだけにする)
  - 2段目より深い共変の位置と、反変の位置の包摂 (D-7。roadmap に書く)
  - E3002 に「内側の矢印のために `Lin` になった」と添える注記 (後の段階)
  - IR の型を1つにまとめること、translate で構造体を開くこと (D-1、D-2)

## 3. 背景の事実

実装する人が知っておく今の状態である。行番号は HEAD 64b14fd7 のもの。

- パスの順番は typed HIR → translate → optimize → EP → `to_lir` (1対1の写し) → boxing → flatten → contract → Perceus → reuse → VM のコード生成である。順番を知るのは `crates/eml_core_ir/src/pipeline.rs` と `crates/eml_lir/src/pipeline.rs` だけである
- 構造体は translate から boxing の後まで `Repr::Struct(S)` の1つの変数で、flatten が葉の変数に開く。flatten の後も `LirFn::source` (`SourceSig`) が開く前の ABI を持つ。VM がそこからシグネチャと構造体と箱の間の変換のフレームを作るためである (docs/spec/lir.md の「開く」、docs/spec/vm.md の「シグネチャ」)
- 関数の ABI は具体化した型と役割 (`Role`: Typed、Body、Clause、Abort、Return、Continuation) で決まる。`apply` の場所は `Typed` か `Uniform` で、合わないときは VM が実行時に変換する。速い道は引数の数と詰めたシグネチャの鍵の一致で、それ以外 (16個以上の引数、複数の値の結果、葉が1つでない構造体) は遅い道で `Class` を比べて変換する。変換のフレームはスカラー用の6つと、構造体の形ごとの組である
- `bench/` のプログラムはどれも `conversions: 0` で、`crates/eml_vm/tests/bench.rs` の `typed_benches_do_not_convert` (1131〜1141行) がそれを確かめている。遅い道を通るのは、手で書いた LIR (`crates/eml_vm/tests/conversions.rs`) と多相再帰の UI テストだけである
- 型どおりのクロージャが得たもの: `apply_float` −43.3%、`apply_int` −33.5%、`ep_float` −17.8% (VM の命令数)。代わりに `apply` のないプログラムが 0.2〜1.7% 増えた (docs/implementation/benchmarks.md の「呼び出しの規約の引き換え」)
- roadmap は、結果の変換が要る末尾呼び出しを Cranelift での「未決の危険」としている (docs/future/roadmap.md の「ネイティブ化」)
- `eml run` の `max_depth` は 1 << 27 語である (`crates/eml_cli/src/lib.rs` の `default_limits`、docs/spec/vm.md の「上限と割り込み」)。lib の既定は `usize::MAX`
- 矢印の線形性は Repr、`Signature`、`FnEntry`、`apply` の場所を変えない。実行時に届くのは、印 (`unr`/`lin`/`poly`) による dup と decref の可否と、`lin` の印のクロージャを作るときのヘッダの `lin` ビット (`crates/eml_vm/src/compile/emit.rs:524`、`:617`) だけである。`lin` ビットがないのは安全な側である (docs/spec/object-model.md:252〜255)
- 関数型の単一化 (`unify_arrow_lin`、`crates/eml_types/src/table/unify.rs:112〜125`) は線形性を等しくし、`Unr ≤ Lin` の枝を持たない。今 `once` の位置に置けるのは、ラムダ、トップレベルの関数、extern、コンストラクタ、クラスのメソッド、REPL の `let` の参照だけである (`fresh_outer_lin`、docs/spec/types.md の「トップレベルの関数の参照」)。2回使った局所変数や引数を渡すと E3002、`->` のフィールドから取った値を渡すと E2001 になる
- `Mark::compatible` (`crates/eml_lir/src/lib.rs:914`) は `(unr, lin)` と `(lin, unr)` の両方を拒む対称な関係である
- `crates/eml_types/tests/scaling.rs` の ignored のテストのうち4つ (`a_ring_of_functions`、`a_ring_of_higher_order_functions`、`a_ring_of_functions_with_local_lambdas`、`many_functions_over_a_long_arrow_chain`) は HEAD で失敗する。生成器が `-` が `Panic` を起こすようになる前に書かれ、生成したシグネチャに `<Panic>` がなく E2002 になるためである
- データの配置: 語は 8 バイトで、下位ビット 1 が即値 (63 ビットの Int、enum のタグ、unit、関数) である。物体はヘッダ 8 バイトの後に、走査する語 (`obj`/`tobj`) を宣言の順に、続けて生の語を宣言の順に並べる。フィールドはどれも1語である。配列の要素はどれも一様な `tobj` で、`Float` は箱に入る。構造体はメモリ上の形を持たず、一様な位置と配列と別の `data` のフィールドでは箱に入る

## 4. 決定

### 4.1 長期の方針 (変えるには見直しが要る)

実装中にこれと食い違ったら、止めて報告する。

#### D-1: IR の層とパスの順番は変えない

- 決定: 2つの IR (型で分けた Core IR と LIR)、1対1の `to_lir`、上の順番のパスをそのまま保つ。複雑さの元は IR の層ではなく、関数の値の動的な呼び出し規約と、それが flatten の後まで残させる構造体の情報である。これは D-3 で取り除く
- 理由: 型が「EP の後に効果の命令がない」「boxing の前に RC と箱がない」を保証している。IR を1つにまとめると、約9つのパスに届かない枝が要り、減る行 (lowering 155、liveness 73、tail 50) より多くなる
- 残す重複: 構造体を決める2つの道が `MAX_STRUCT_WORDS` を共有すること、推移的でない Repr の互換、スカラーの箱、2つの liveness と2つの末尾の位置の判定 (EP が Core IR のものを要る)、Core IR と LIR のテキストが同じであること (テストのある約束)、`eml_lir::check` を共有する2つの verifier
- 退けた案: IR の型を1つにまとめる。translate (7.3k 行) と optimize (6.5k 行) を割る (大きさは単相化、決定木、handler の特殊化、融合によるもので、IR の欠陥ではない)
- 見直す条件: ネイティブ化のバックエンドが LIR のほかの形を要るとき

#### D-2: 構造体は flatten まで1つの変数のまま扱い、translate では開かない

- 決定: 今のとおり、構造体を boxing の後の flatten で葉に開く。docs/spec/core-ir.md の「構造体にする型」に、この理由の1文を書く
- 理由: D-3 で `SourceSig` がなくなると、flatten の後に構造体のまとまりを要るものはなくなる。flatten の前は、boxing (一様な位置への `box s`、`$uniform` の構造体の引数の `unbox`、覗き穴) と flatten (構造体の `box`/`unbox` を `con`/`UnboxStruct` に展開すること) がまとまりを要る。translate で開くと、translate の式と pattern の結果 (expr.rs 1313 行、pattern.rs 1041 行) がすべて値の並びになり、boxing が葉からまとまりを組み直すための情報を optimize と EP の間ずっと持つことになる。行数はほぼ同じで、複雑さが散らばる。得られるのは optimize の状態を渡す複製の組を複数の値にすることだけで、それは今も確保を生まない (docs/spec/core-ir.md:651〜658)
- 確度: 中。試作はしていない
- 見直す条件: optimize か EP で、構造体の葉ごとの書き換えが要る最適化を入れるとき

#### D-3: 関数の値の ABI は、捕まえた値より後ろをいつも一様にする

- 決定 (関門 G の条件付き。D-14):
  - 直接の呼び出しは今の型どおりの ABI を保つ。構造体の結果は複数の値の `CallN`/`RetN` のままにする
  - 値として生まれる関数 (ラムダ、`handle` の本体、節、`return` の節、`f$kN`、`cont$`、`op$`、`con$`、`$externN`、それらの optimize の複製) は、捕まえた値の数を `split` として持つ。`split` より前の引数 (捕まえた値) は型どおりの Repr、`split` 以降の引数と結果は一様にする。今の `Role` は EP が渡す後ろの位置でこれと同じことをしており、`Role` の6つの種類は作るときに決まる1つの整数になる
  - 値として参照する型どおりの関数 (`&f`、トップレベルか `$let$` の関数の `closure f(..)`) には、名前で決まる `f$uniform` (すべて一様な ABI) を作る。この計画では `f$uniform` を版と呼ぶ。形は D-12 で決める
  - `apply` の場所はどれも一様にし、boxing が普通の位置の規則で `box`/`unbox` を入れる
  - 実行時の変換をすべて消す: `Site`、`source_ret`、`Role`/`conform_role`、VM の `Signature`/`key`/`Class`、`apply_slow` の変換、変換のフレーム、末尾の合成、`TailCall` の結果の変換の operand、`ApplyN`、`CompileError::NoConversion`、`RunStats.conversions`、`LirFn::source`/`SourceSig`。`apply` は引数の数を比べ、語を写して跳ぶか、部分適用か `ApplyRest` にするだけになる
- 理由: 動的な規約が効くのは手で書いた LIR と多相再帰のテストだけである。消すのは約 1,350 行 (VM の sig.rs 431、apply.rs 約250、vm.rs 約80、op/disasm/code 約100、emit.rs 約50、LIR の flatten 約100、verify 約80、syntax/text/pretty 約120、boxing の conform_role 約30、Core IR の `Site` 約100、`Role` 約50) で、足すのは約400行である。vm.md の「シグネチャ」「変換のフレーム」と lir.md の「`apply` の場所」がなくなる。ネイティブ化では `return_call` をそのまま使え、関数ごとの入口は1つになり、コードのキャッシュの鍵は自分の LIR だけになる。roadmap の「未決の危険」は仕組みごとなくなる
- 引き換え (利用者が承認済み): 特殊化されなかったクロージャに `Float` や構造体を渡すと、呼ぶたびに箱ができる。見積もりは `apply_float` が VM の実行の命令数 +76%・確保 +20 万、`apply_int` が +50% (シフトだけで確保はない)。特殊化されたが展開されなかったラムダの直接の呼び出しも引数を箱に入れる (値として生まれる 724 個の関数のうち 103 個が直接も呼ばれる)。EP の道 (`ep_float`、handler) は変わらない
- 退けた案:
  - 今の規約を保ち、フレームの合成を Cranelift に移す (複雑さが残る)
  - すべての関数の値を薄い包みにする (末尾呼び出しの保証を失い、2^27 語の `DepthLimit` に届く)
  - `Role` を消して値として使うすべての関数に `$uniform` を作る (値として生まれる関数の一様さを複製で作り直すことになる。値として生まれるのが 724 個、包みが要るのは 71 個と測った)
  - 結果の変換の種類ごとの版 `f[ret=int]` (場所のシグネチャごとに要り、合成の問題が戻る)
  - すべての関数の結果を一様にする (`Float` を返すたびに確保する)
- 見直す条件: 関門 G を通らないとき。そのときは今の規約を保つ

#### D-4: データの配置は eml 独自のものを保ち、C とは境目で変換する

- 決定: 一般の物体の配置を C に寄せない (`repr(C)` の data、1語より小さいフィールド、宣言の順のフィールド、C の形の構造体の型を作らない)。C との受け渡しは、std の extern が今使う境目 (Repr で型を決めた 64 ビットの語のスカラー、文字列、`Foreign`) と、長さの語の後に連続した中身を置く物体 (今は `String`) で行う。配列の物体の形は要素の型によらない。この方針を docs/future/roadmap.md の「ネイティブ化」に書く
- 理由: Cranelift にとって今の配置は障害にならない。オフセットは配置ごとに決まった定数で、生の葉はそのまま `i64`/`f64` である。C と食い違うのはヘッダ、参照のフィールドを前に寄せる順番、1語のフィールド、一様な位置のタグ付きの Int と箱の Float、構造体にメモリの形がないことで、どれも配置を型の具体化から独立させるためのものである
- 退けた案: 要素の型で形を変える `Array a` (OCaml の平らな float 配列。一様なコードのすべての読み書きに種類の確かめが要る)、型引数で選ぶ Repr ごとの配列の extern、C の形の一般の配置、NaN-boxing
- 見直す条件: ネイティブ化で、一様な位置の箱がホットパスで目立つとき

#### D-5: 構造体のフィールドへの埋め込みと、スカラーを生で持つ配列は見送る

- 決定: どちらも今は作らない。roadmap の2つの項目に、次の見直す条件と、そのときの設計の要点を書く
- 理由: `Float` の配列の箱が確保の大半を占める数値の bench (nbody、spectral-norm、mandelbrot) はすでに Python の 0.4〜0.6 倍で、1.5 倍の条件に入っている (benchmarks.md の「今の数値」)。1.5 倍を超える fannkuch-redux と sieve の要素は `Int` と `Bool` の即値で、箱に入らない。構造体の型のフィールドを持つコンストラクタは `bench/` と `std/` に1つもなく、UI テストの6つだけにある。親が構造体なら入れ子の構造体はもう葉に並んでいる。nbody の箱は配列の要素で、フィールドへの埋め込みでは減らない
- 見直す条件:
  - 生の配列: ネイティブで測った `bench/` の数値のプログラムが Python の 1.5 倍を超え、その `RunStats` の `boxes` が `allocations` の半分を超えるとき (今の spectral-norm は 4043 のうち 4001)。または FFI で `double*` が要るとき
  - フィールドへの埋め込み: `bench/` か `tests/ui/run/scripts/` のプログラムで、構造体でないコンストラクタの、構造体の型のフィールドの箱が `allocations` の 10% を超えるとき
- そのときの設計 (roadmap に要点を書く):
  - 生の配列は `Array a` と別の型にし、長さの語と連続した中身を持つ物体の仲間 (`String`、`Bytes`) にする。`Bytes` を作るときに、要素の数を長さの語に持つ形にしておくと、平らな `Float` の配列はそれを 8 バイトの要素にしたものになる
  - 埋め込み: 配置のフィールドの Repr を、そのフィールドの型の scheme が構造体の型で配置自身の型変数を含まないときに `struct(S)` にする。`Shape` は `struct(S)` のフィールドを深さ優先で葉に展開してから、今の規則 (参照を前、生を後ろ、`unit` の葉は0語) で並べる。flatten の前は1つのフィールドを1つの構造体の変数で運び、flatten が展開する。配置の表は REPL の追記のために中身で引く。組の配置は型変数のフィールドなので埋め込まない

#### D-6: ユーザーの FFI は最小の形を将来の方針として決め、今は作らない

- 決定: ネイティブ化で `extern "C"` を足すときに、利用者のモジュールで次の形だけを許す。引数と結果はスカラー (`Int` = `int64_t`、`Float` = `double`、`Bool` と enum = `int64_t`、`Unit`)、借りて (ポインタ, 長さ) の組で渡す `String`/`Bytes`、不透明な C のハンドル。C の構造体 (値でもポインタでも)、出力の引数、コールバック、eml の物体の配列は扱わない。それを超えるものは今と同じく Rust で書いた extern の行にする。C のハンドルは `Foreign` の包み `CHandle { ptr: *mut c_void, dtor: Option<unsafe extern "C" fn(*mut c_void)> }` に `unsafe impl Send` を付けたもので、ほかの `Foreign` と同じく回収できない。中身のポインタは `eml_rt` のアクセサで取り、C の側で定数のオフセットを使わない。E1033 は `extern "C"` を足すまで残す。これを roadmap の「ネイティブ化」の `extern "C"` の項目に書く
- 理由: 今の語の ABI (引数の数が決まった 64 ビットの語、`borrowed`、長さの語を持つ物体、ファイナライザを持つ `Foreign`) で、この最小の形に要るものはそろっている
- 見直す条件: C の構造体やコールバックを要する std の外のライブラリを使うとき

#### D-7: `Unr` の矢印を `Lin` の矢印の場所に渡せるようにする (読む式で広げる)

- 決定: 単一化は変えない。値を読む式の型で、`Lin` でない一番外側の矢印を、元の線形性以上の新しい Kind 変数に替える (規則 W)。型構成子の共変の引数と組の要素にも1段だけ同じことをする。未決の変数を関数型の期待と比べるときは、一番外側の線形性だけを期待以下の新しい Kind 変数にした型に決める (規則 F)。逆向き (`Lin` を `Unr` の場所に渡すこと) はどこでも許さない。2段目より深い位置、反変の位置、関数型の引数の包摂は roadmap に置く
- 理由: 実行時の構造で矢印の線形性を持つものはない。`lin` ビットがないのは安全な側で、reuse は実行時に一意かを確かめ、`apply` は `is_last` を確かめる。多重の継続は厳しい側の見方しか見ない。制約は今と同じ `≤` の原子で、SCC ごとの最小解で解ける。試作で、`run` と `run-fail` の UI テストは両方のモードで通り、bench の `RunStats` は変わらず、LIR の関数は 3308 から 3306 になり、`lin` のクロージャは 134 のままだった。scaling の13の形は比 4.03〜4.13 で HEAD と同じで、絶対時間は +0〜7% だった
- 退けた案: 不変のまま (ラムダで包むことを利用者に求め続け、局所変数とトップレベルで扱いが違う)、`expect` の中だけで包摂する (順番によって結果が変わり、`Some g` と `pair g g` が通らない)、深さに制限のない共変の拡張 (読むたびに型の大きさに比例する写しが要り、2乗の時間になりうる)、極性を持つ単一化の器 (すべての `unify` に向きが要る)、IR に何もしない変換の節点を足すこと
- 見直す条件: 入れ子の関数のコンテナを作り直す書き方が繰り返し要るとき、またはネイティブのコード生成が要るとき

### 4.2 短中期の設計

理由があれば実装中に変えてよい。変えたら記録する。

#### D-10: 規則 W と規則 F の細部

- 読む式は次のすべてである: 局所変数、引数、pattern の変数、`var` を読む式、トップレベルの関数と値、extern、コンストラクタ、クラスのメソッド、操作、REPL の `let` の参照、射影 `e.x`、添字の読み出し `e[i]`、呼び出しの結果 (飽和でも部分適用でも)
- 操作も W に含める。ece19ba1 が操作を外した理由は書かれておらず、実際に妨げているのは `crates/eml_core_ir/src/translate/instances.rs:767` の debug の「操作の検査」が、参照の型全体を `lin_compatible` で比べていることである。これを `lin_compatible_inner` (関数の参照の検査がすでに使っているもの) に替える
- 広げ方 (試作の形。名前は `Table::widen_outer_lin`、`crates/eml_types/src/table/row.rs` に置く):

```rust
pub fn widen_outer_lin(&mut self, ty: Ty) -> Ty {
    let ty = self.resolve(ty);
    match self.shape(ty).clone() {
        TyShape::Con(def, args, rows) => {
            // 共変の引数だけを1段広げる。変位は data の Kind の表にある
            let variance = self.data_kinds()[def].variance.clone();
            let mut changed = false;
            let args: Vec<Ty> = args.iter().enumerate().map(|(i, &a)| {
                if variance.get(i) == Some(&Variance::Co) {
                    let w = self.widen_arrow(a);
                    changed |= w != a;
                    w
                } else { a }
            }).collect();
            return if changed { self.alloc(TyShape::Con(def, args, rows)) } else { ty };
        }
        TyShape::Tuple(elements) => { /* 各要素に widen_arrow。変わったときだけ作り直す */ }
        _ => {}
    }
    self.widen_arrow(ty)
}

fn widen_arrow(&mut self, ty: Ty) -> Ty {
    // 関数型で、一番外側が Known(Lin) でなければ、fresh >= lin の新しい Kind 変数に替える
    // require_lin(bound(lin), bound(fresh)); function_with(param, fresh, row, ret)
}

pub fn flow_into(&mut self, found: Ty, expected: Ty) -> Result<(), UnifyError> {
    // found が未決の変数で expected が関数型なら、fresh <= expected.lin の新しい Kind 変数を
    // 一番外側に持つ写しに found を決める。それ以外は unify(found, expected)
}
```

- 当てる所 (`crates/eml_types/src/check/`): `BodyCheck::path` の `Res::Local` と item (`ValueItem::Operation(_) => ty` の枝を消す)、`var_get`、`ExprKind::Field`、`ExprKind::Index`、`BodyCheck::call` の結果 (`open_spine` と期待との比べの前)。`BodyCheck::expect` の `unify` を `flow_into` に替える。`fresh_outer_lin` を消し、`crates/eml_types/src/` の store.rs:423 のコメントを直す。`require_lin` を `pub(crate)` にする
- 順番の条件: W は、読む時点で型が分かっているときに効く。これは spec が決めている双方向の検査の順番である。F は未決の変数を関数型と直接比べるときを受け持つ。それ以外 (未決の変数を形が分かる前に `Option (once …)` と単一化するときなど) は順番で結果が変わることがあり、引数に型を書けば避けられる。これを types.md に書く
- `var` は今のとおり `Unr` でなければならない (E3007)。`var` が持つ `Unr` の関数を読むと W で広がるので、`once` の位置に渡せる
- E2001 は残る。定数の `Lin` の矢印は広げないので、`Lin` を `Unr` のフィールドに入れると今のとおり E2001 になる

#### D-11: 印の向きのある比べ方 `Mark::passes_to`

- `Mark::passes_to(sender, receiver)` を足す。偽になるのは `lin → unr` だけで、`poly` はどちらの向きも通す。対称な `compatible` は、対称な使い方が残らなければ消す
- 呼ぶ所の分類 (B-2 の監査の結果):
  - 値の流れ。`passes_to(送り手, 受け手)` に替える:
    - `crates/eml_lir/src/check.rs:94` (操作の引数 → 節の引数。今は受け手が先)
    - check.rs:111 (節の結果 → 操作の結果)
    - check.rs:493 `mark_passes` (atom → 受け手。呼び出しと `apply` の引数、`jump`、`return`)
    - check.rs:938 `bound_mark` (呼び出しの結果 → 束縛する変数。今は受け手が先)
    - `crates/eml_lir/src/verify.rs:1966` (末尾呼び出しの結果 → この関数の `ret`)
    - `crates/eml_core_ir/src/optimize/fuse/known_con.rs:87` `passes`
    - `crates/eml_core_ir/src/optimize/handlers/passing.rs:273`
    - `crates/eml_core_ir/src/optimize/handlers/mod.rs:705`
  - 合成の流れ (間の変数を消して直接の組を比べる変換)。直接の組に `passes_to` を使う:
    - `crates/eml_lir/src/contract.rs:86` (呼ばれる側の `ret` → 呼ぶ側の `ret`)
    - `crates/eml_core_ir/src/optimize/fuse/inline.rs:298` `returns_pass`
    - `crates/eml_core_ir/src/optimize/handlers/mod.rs:552` `returns_handled`
  - 置き換え (変数を別の変数に替える)。変えない。どれも `compatible` を使っていない:
    - `inline.rs:239` と `crates/eml_core_ir/src/optimize/blocks.rs:214` (印が等しいこと)
    - `handlers/mod.rs:704`、`:1026` (`at_least`)
    - translate の `jump` の転送 (印が等しいこと。core-ir.md:232)
- 結果として、`unr` の `g` を渡す `run_once g` は `run_once` を展開しない。bench の `RunStats` での費用はなかった
- V2 は `unr → lin` を確かめなくなる。代わりの検査は足さない。translate で作ったクロージャを無理に `unr` にする変異の実験で、作る場所の検査 (`check_built`) が止めることを確かめた。確かめなくなる向きは、`lin` ビットがなくなるだけで、それは安全な側である
- D-3 との組み合わせ: `f$uniform` は `f` の印を写すので、`f` を呼ぶ所は印の等しい値の流れである。D-13 の末尾の規則は合成の流れなので、`passes_to(呼ばれる側の ret, 呼ぶ側の ret)` を使う。一様な `apply` の引数には比べる受け手の印がない

#### D-12: `split` と `f$uniform` の作り方

- `LirFn::split: u32` が `role` に代わる (Core IR も同じ)。translate がラムダ、本体、節、`return` の節、`cont$`、`op$`、`con$`、`$externN` に、捕まえた値の数を入れる。EP が `f$kN` に生きている変数の数を入れる。optimize の複製は複製元の `split` から開いた後ろの位置を引いた値にする (捕まえた値の数より下げない。core-ir.md:718 の規則をこう書き直す)。型どおりの関数は「なし」(`Option` か `arity + 1`) にする。テキストの形は `uniform from N` とする
- 値として生まれた関数でも、optimize の後に値の参照 (`&f` と `closure f`) が1つも残らないものは、型どおりの ABI のまま (`split` はなし) にする。決めるのは Core IR (optimize の終わりか lowering) である。ラムダを参照するのは親の関数のコードと、その名前を引く optimize の複製だけなので、名前が同じならコードと ABI も同じになる。T-6 の実装中に、`ep_float` の繰り返しの中のラムダが直接呼ばれるだけなのに結果を一様にされ、反復ごとに箱を作ったことから、利用者と決めた
- EP の合流の関数 `f$jJ` は継続のコードからしか呼ばれないので、一様な結果にする
- 用語: 結果が一様な関数とは、`rets` が `{obj, tobj, unit}` の1つの Repr の関数である (値として生まれる関数、すべての `$uniform`、`f$jJ`、EP の補助の関数、結果が `_` の `_` の instance、結果の型がそれらの Repr の関数)。それ以外を結果が型どおりの関数と呼ぶ。末尾の位置の呼び出しは lir.md:274 の形の定義 (`apply`、`clause`、`resume` を含む) で、boxing の前に決める
- `needs_copy(g)`: `g` から、結果が型どおりの関数への末尾の位置の直接の呼び出しをたどって届く範囲に、末尾の位置の `apply`、`clause`、`resume`、または結果の変換が要る直接の末尾呼び出しがあれば真。呼ばれる側の本体だけで決まり、本体は名前で決まる
- `f$uniform` の形:
  - 薄い形 (`needs_copy(f)` が偽): 引数を `unbox` し、`let r = call f(..)`、結果を `box` して返す
  - 写しの形 (`needs_copy(f)` が真): `f` のブロックと変数の表を写し、結果が型どおりの関数 `g` への末尾の位置の直接の呼び出しをすべて `g$uniform` に替え (`g` を作業の列に足す)、すべて一様にそろえる
- 結果が一様なすべての関数 (値として生まれる関数と `$uniform`) の中でも、結果が型どおりの `g` への末尾の位置の直接の呼び出しを `tail call g$uniform(箱に入れた引数)` に替える
- boxing の手順:
  1. 値の参照を集める。値として生まれる関数はその場でそろえる (今の `conform`、boxing.rs:196 の位置の並びを `split` から作る。`conform_clause` は変えない)
  2. boxing の前のプログラムで `needs_copy` を求める
  3. `f$uniform` を作り、`&f`/`closure f` をそちらに向ける
  4. 結果が一様な関数の中の末尾の振り替えを、新しい版が出なくなるまで作業の列で繰り返す
  5. その後の関数ごとの手順は今と同じで、版も含むすべての関数にかける。読むのは呼ばれる側のシグネチャだけで、それは手順4の後に決まっている
- 型どおりの元の関数は参照されなくても残す (REPL が名前を保つため。消すのは optimize の仕事)
- verifier: `<f>$uniform` という名前の関数は `<f>` があるときだけ存在し、一様な ABI を持つ。debug ビルドでは `<f>` から作り直して同じテキストになることを確かめる (REPL の `Mismatch` の確かめと同じ仕組み)
- 版の情報: `yields`、`internal`、文の位置 (`Loc`) は `f` から写す。印も写す。`split` はなし、`clause` の印は付けない。薄い形の `call f` には yield の確認がない。yield 中は `f` がダミーを返し、スカラーのダミーの `box` はダミーを返す (lir.md:409)。構造体の結果はダミーの葉の配置の物体に入る (yield の道で1回確保し、呼ぶ側の `drop x` が放す)。そのフレームは κ に入らず、再開した値は κ を通って一様な `apply` の場所に一様な値として届く (core-ir.md の「yield の確認のない呼び出しの後」の約束)。巻き戻しの表は版の `call f` にも普通の場所と同じく作る。実行時エラーの位置は `f` の位置のままで、版の名前は逆アセンブルと後の backtrace にだけ出る
- クロージャの物体の形は今のまま (`split` より前の捕まえた値は Repr のとおり走査する語か生の語、後から足す引数は一様な語)。VM の部分適用は、足す引数が一様なのでシグネチャなしで語を写す。docs/spec/object-model.md の「クロージャ」はこの部分だけ書き直す

#### D-13: 静的な末尾の規則と、保つ保証

- 末尾の位置の呼び出しは、結果の Repr が関数の `rets` と互換で、印が `passes_to` を満たすときだけ `tail` にする。boxing の末尾の位置の例外 (lir.md:462) と、boxing の `tail_position_call` の使用を消す
- 保証: 末尾の位置の呼び出しの鎖ごとに、残るフレームは多くて1つである。フレームが残るのは、結果が型どおりの関数が結果の一様な関数を呼ぶ所だけで、そこから末尾でたどる先はすべて結果が一様なコードであり、結果が一様なコードは結果が型どおりのコードを末尾で呼ばない (振り替えがあり、`apply` の結果は `tobj` で、型の規則がそれを妨げる)
- 例外として lir.md の「末尾の位置の呼び出し」に書くもの: `mask` の包みと `prompt$` (今と同じ)、操作の表の結果が節の関数の結果と違う節の入口 (同じ操作を節の中で起こすと外側の handler に行くので、深さは handler の入れ子で抑えられる)、結果が一様なコードの中の、操作の表の結果が箱を要る `tail clause` (in-place の入口が戻るまでの一時的なもの)
- 今ある `crates/eml_vm/tests/scaling.rs:419〜460` の `a_loop_through_a_function_value_that_returns_{int,bool,unit}_keeps_the_stack_and_heap_flat` はまさにトランポリンの形で、そのまま通らなければならない

#### D-14: 関門 G と、その閾値

- 時期: T-5 を入れた状態で、T-7 (古い仕組みを消す) の前
- 測るもの:
  - `cargo test -p eml_vm --test integration bench::` の `RunStats`
  - `bench/run.sh`
  - `bench/scripts.sh` (実時間と最大 RSS)
  - `bench/` と `tests/ui/run*` の LIR の文の総数と、版と写しの数 (T-5 で足す開発用の数え方。7.2 を参照)
  - `cargo test` の実時間
  - T-5 で足す scaling の試し
- 通らない条件 (どれか1つで通らない):
  - `apply_float` の VM の実行の命令数が +80% を超える。または確保が今の値 + 200,020 を超える
  - `apply_int` が +55% を超える。または確保が1つでも増える
  - `ep_float` が +5% を超える
  - ほかの bench のプログラムの命令数が +2% を超える
  - スクリプトの実時間が +3%、または最大 RSS が +5% を超える
  - LIR の文の総数が +2% を超える
  - `cargo test` の実時間が +5% を超える
  - scaling の試しが平らでない
  - LCG の試しの1回の反復あたりの確保が 2 を超える
- 決め方: 測った数値を記録して利用者に見せ、利用者が承認するか止める。止めたら T-4 は残し (振る舞いを変えない整理)、T-5 を戻す。`scripts/csv2jsonl` の行だけが通らないときは、T-11 (C1) を先に入れて測り直す

#### D-15: docs の置き場所

- docs/spec は今の状態だけを書く。将来の配置 (`Bytes`、平らな `Float` の配列、ユーザーの FFI、`CHandle`、配列を常に要素の数の語を持つ形にすること) は docs/future/roadmap.md に置く。docs/spec/object-model.md と docs/spec/stdlib.md に将来の文は足さない
- 日本語の文は yomiyasu スキルに従って書く (CLAUDE.md)

## 5. 実装の約束

- 型の規則の変更 (T-4) は印の向きの変更 (T-3) と同時かその後に入れる。型だけを変えると、debug ビルドで V2 が panic する (c4 の試しで確かめた) (D-7、D-11)
- 名前が同じ関数は同じコードにする。`f$uniform` の形は `f` のコードだけで決め、プログラムのほかの部分を見ない (D-3、D-12)
- 版を作るのは boxing だけである。optimize と EP は `split` を付けるだけにする (D-12)
- docs/spec の節の名前を変えたら、docs と code の中の参照も直す。とくに types.md の「トップレベルの関数の参照」を「値を読む式の矢印の線形性」に変えるときは、`grep -rn "トップレベルの関数の参照" crates docs std tests` を空にする (D-10)
- テストの変更は7.3の種類ごとの一覧にあるものだけにする。一覧にない種類1と種類2の変更が要ったら、止めて報告する (CLAUDE.md の Testing)
- eml_rt は変えない。クロージャは `eml_vm` が `alloc_data` で作る普通の `Data` で、`crates/eml_rt/src` にクロージャのコードはテストの外にない。Verus の関門には当たらない (D-12)

## 6. 実装のタスク

| ID | タスク | 基づく決定 | 前提 | 並べてよいか |
|---|---|---|---|---|
| T-1 | roadmap と core-ir.md に、変えない論点の結論と見直す条件を書く | D-2、D-4、D-5、D-6、D-15 | - | T-2 と並べてよい |
| T-2 | scaling の生成器を直す | D-7 | - | T-1 と並べてよい |
| T-3 | `Mark::passes_to` | D-11 | - | いいえ |
| T-4 | 規則 W と F、spec、UI テスト | D-7、D-10 | T-2、T-3 | いいえ |
| T-5 | `Role` を `split` にする整理 | D-3、D-12 | T-4 | いいえ |
| T-6 | 切り替え (値として生まれる関数の一様化、`$uniform`、一様な `apply`、静的な末尾の規則) | D-3、D-12、D-13 | T-5 | いいえ |
| T-7 | 関門 G の測定と利用者の判断 | D-14 | T-6 | いいえ |
| T-8 | 動的な仕組みを消す | D-3 | T-7 の承認 | いいえ |
| T-9 | 測り直しと docs | D-3、D-13 | T-8 | いいえ |
| T-10 | LIR のパスのテストを eml_lir に移す (任意だったが、利用者が含めると決めた) | D-1 | T-9 | はい |
| T-11 | (条件付き) `<g>{k}` の複製を既知の継続で特殊化する | D-14 | T-6 | いいえ |

T-3 と T-4 を T-5 より先に置くのは、T-6 の末尾の規則が `passes_to` を使うからである。

### T-1: 変えない論点の結論を docs に書く

- 目的: 調べて変えないと決めた論点の結論と見直す条件を、次の人が同じ調べ直しをしないように残す
- 対象: docs/future/roadmap.md、docs/spec/core-ir.md
- 作業:
  - roadmap の「ネイティブ化」の `extern "C"` の項目に、D-6 の最小の形と、扱わないものと、`CHandle` を書く。同じ節に、D-4 の方針 (独自の配置を保ち、境目で変換する。配列の形は要素の型によらない) を書く。静的なデータでの検査モードの範囲の確かめ (roadmap の今の項目) は、起動時の `freeze` を既定にすると書く
  - roadmap の「スカラーの要素を生で持つ配列」と「構造体のフィールドを親の物体に並べること」に、D-5 の「後に回した理由」「見直す条件」と、そのときの設計の要点を書く
  - roadmap の「ネイティブ化」に、配列を常に要素の数の語を持つ形 (`scan = 255`) にする案を、ネイティブのコードが配列の要素を直接読むコミットで行うと書く (`crates/eml_rt/src/heap/ctx/arrays.rs` の Verus の仕様の6か所が変わる)
  - core-ir.md の「構造体にする型」に、構造体を flatten まで1つの変数で扱う理由を1文で書く (D-2)
- 完了の条件: `cargo test -p eml_cli --test integration ui::` が変わらず通る。docs だけの変更である
- 落とし穴: docs/spec に将来の文を書かない (D-15)

### T-2: scaling の生成器を直す

- 目的: HEAD で失敗する4つの ignored のテストを、意図どおりの入力に戻す。T-4 の費用をこの形で測るために要る
- 対象: `crates/eml_types/tests/scaling.rs`
- 作業: 4つの生成器 (`ring`、`ring_of_higher_order_functions`、`ring_with_local_lambdas`、`functions_over_an_arrow_chain`) が作るシグネチャで、`-` を使う関数の結果に `<Panic>` を付ける。たとえば `f{i} : a -> Int -> a` を `f{i} : a -> Int -> <Panic> a` に、`q : Unit -> (Int -> Int)` を `q : Unit -> <Panic> (Int -> <Panic> Int)` にする。合否の条件 (比 ≤ 6) は変えない
- 完了の条件: `cargo test --release -p eml_types --test integration scaling:: -- --ignored` で13件がすべて通る
- 種類: 2 (古くなった入力を直す。7.3)

### T-3: `Mark::passes_to`

- 目的: 印の比べ方に向きを持たせ、`unr` の値を `lin` の受け手に渡せるようにする (D-11)
- 対象: `crates/eml_lir/src/lib.rs`、D-11 の値の流れと合成の流れの各所、docs/spec/lir.md の「Kind の印」「縮約」と V2 の行、docs/spec/core-ir.md:85、:97、:138、:452 (「食い違いがないことの検査」。作る場所の検査が網になることを書く)、:688
- 作業: D-11 のとおり。次のテストを足す
  - `crates/eml_lir/tests/contract.rs`: 縮約が `unr → lin` で末尾呼び出しを作り、`lin → unr` では作らない
  - `crates/eml_core_ir/tests/` (inline と known_con の既存のファイル): `returns_pass` と `passes` が `unr → lin` を通し、`lin → unr` を拒む
  - `crates/eml_lir/tests/verify.rs`: V2 が各境目で `lin → unr` を拒む
- 完了の条件: `cargo test`。今の正しいプログラムは `unr → lin` を作らないので、UI の出力は変わらない
- 種類: 7.3 の T-3 の行

### T-4: 規則 W と F、spec、UI テスト

- 目的: D-7 を入れる
- 対象: `crates/eml_types/src/check/body.rs`、`check/var.rs`、`table/row.rs`、`table/mod.rs`、`crates/eml_core_ir/src/translate/instances.rs:767`、docs/spec/types.md、docs/spec/effects.md:126、docs/spec/core-ir.md の「食い違いがないことの検査」と :456 の参照、docs/future/roadmap.md:127、docs/future/verification.md:69、UI テスト
- 作業:
  - D-10 のとおりにコードを変える
  - types.md の「関数型」(107行) を W と F で書き直す (「1段だけ、共変の位置だけ」を含める)。「トップレベルの関数の参照」(214〜230行) を一般の規則にし、節の名前を「値を読む式の矢印の線形性」にする。`get_worker`/`Poly` の段落は残す。操作と局所の関数を外す文と、ラムダで包むよう求める文を消す。D-10 の順番の条件を書く。「変数の分類」に `var` を読む式の1文を足す
  - effects.md:126: 節は引数を何度使ってもよく、`once` の位置に渡してもよいと書く。宣言の側で `Unr` に決める規則は残す
  - core-ir.md: 操作の検査が `lin_compatible_inner` で比べると書く
  - roadmap.md:127 を「関数型の深い包摂 (2段目より深い共変の位置、反変の位置、関数型の引数)」に替え、後に回した理由 (極性を持つ単一化の器か、型の大きさに比例する読むたびの写しが要る) と見直す条件 (D-7) を書く
  - verification.md:69: 制約の形は変わらないと書き直す
  - UI テストは7.3の T-4 の行のとおり
- 完了の条件:
  - `cargo test`
  - `cargo clippy --all-targets -- -D warnings` と `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings`
  - 新しい run のテストを1つずつ `cargo run -q -p eml_cli -- run --debug-heap <file>` で動かし、期待どおりの出力になる
  - `cargo test --release -p eml_types --test integration scaling:: -- --ignored` で13件が通る
  - `cargo test -p eml_vm --test integration bench` の `RunStats` が変わらない
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` が通る
  - `grep -rn "fresh_outer_lin" crates docs` が空になる
  - `grep -n "ラムダで包んで渡す\|ラムダで包む (\`Task" docs/spec/types.md` が空になる
  - `grep -rn "トップレベルの関数の参照" crates docs std tests` が空になる
- 落とし穴: 呼び出しの結果を広げるのは `open_spine` の前である。操作の参照の枝を消し忘れると、`once` の継続の試し (op_once) が E2001 のまま残る

### T-5: `Role` を `split` にする整理

- 目的: 振る舞いを変えずに、役割を1つの整数にする。T-6 の差分を小さくする
- 対象: `Role` を使う所 (Core IR と LIR の型、translate、EP、optimize の役割の写し、boxing の `conform_role`、テキストの形と verifier)
- 作業: EP の役割の関数 (本体、節、`return` の節、`f$kN`) に、今と同じ一様な位置になる `split` を付ける。テキストの形を `role …` から `uniform from N` にする
- 完了の条件: `cargo test`。bench の `RunStats` のスナップショットが変わらない。`grep -rn "Role::" crates --include='*.rs'` が空になる
- 種類: 2 (`role ` を含む IR のテキストのスナップショット。7.3)

### T-6: 切り替え

- 目的: D-3 の規約に切り替える。1つのコミットにする。すべてのクロージャと場所が一様になるので、VM の変換の仕組みは動かず、`typed_benches_do_not_convert` もそのまま通る。古い仕組みはまだ消さない
- 対象: translate (値として生まれる関数の `split` を捕まえた値の数にする)、EP (`f$jJ` の一様な結果)、boxing (D-12 の手順、末尾の例外を消す)、contract と verifier (D-13 の末尾の規則、`tail` の結果と `rets`、`apply` が一様であること、値の参照の規則)、Core IR の `Site` と `Call::Resume` の Repr の並びを消す
- 作業: D-12、D-13 のとおり。次のテストを足す
  - `crates/eml_vm/tests/variants.rs` (新しいファイル。`tests/main.rs` に宣言する): 薄い形と写しの形。薄いフレームを通る yield (スカラーと構造体のダミー)。一様な値を場所に届ける再開。版のフレームを通る巻き戻し
  - `crates/eml_vm/tests/scaling.rs` に、今の `peaks`/`assert_flat`/`HIDE` を使う試し (n = 1000 と 2000、検査モード): data に隠した関数の値から入る `Float` の累積のループ、同じく構造体の累積のループ (`peak_stack`/`peak_objects` が平らで、`allocations` の差が定数の部分と等しい)、CPS の Int の鎖、`<g>{k}` の再開のループ、隠したクロージャを通る 64 ビットの LCG (1回の反復の確保が 2 以下)
  - `crates/eml_cli/tests/repl/` のトランスクリプト: 1つ目の入力で `f : Int -> Int` を定義し、2つ目で `f` を値として使い、3つ目で直接呼ぶ
  - 開発用の数え方 (関門 G で使う): `bench/` と `tests/ui/run*` の LIR の文の総数と、`$uniform` の数と写しの数を数える。置き場所は ignored のテストか `bench/` のスクリプトのどちらかにし、どちらにしたかをコミットに書く
- 完了の条件:
  - `cargo test`
  - `grep -rn "Site::" crates --include='*.rs'` が空になる
  - scaling.rs の平らなスタックのテストが通る (今あるものと新しいもの)
- 種類: 2 (7.3 の T-6 の行)。種類1はまだない

### T-7: 関門 G

- 目的: D-14 の数値を測り、利用者が続けるかを決める
- 作業: T-6 を入れた状態と T-6 の前の状態で D-14 の項目を測り、表にして利用者に見せる。`apply_float`、`apply_int`、`ep_float` の差は個別に書く
- 完了の条件: 利用者が承認するか、止めるかを答えている。止めたら T-6 を戻し、T-8 と T-9 は行わない。そのときも T-1〜T-5 は残す
- コミット: なし (測った表は T-9 で benchmarks.md に書く)

### T-8: 動的な仕組みを消す

- 目的: 使われなくなった実行時の変換を消す (D-3)
- 対象: eml_vm (`sig.rs`、`Class`、`convert*`、`tail_convert`、`convert_return`、`convert_struct_return`、`Receive`、`RetBox`/`RetUnbox` の3組、`RetBoxStruct`/`RetUnboxStruct`、`Conversions`、`CodeBase::structs`/`signatures`、`ApplyN`、`NoConversion`、`RunStats.conversions`、逆アセンブルの変換とシグネチャの表示)、eml_lir (`LirFn::source`/`SourceSig`、`Call::Apply::source_ret`、開く前の ABI を確かめる verifier の検査と文言 (lir.md:680〜685)、`uniform apply` と `source(…)` の構文、flatten の記録)。`FnEntry` は `{ name, start, end, arity, frame_size }` にする。`ApplyRest` の残りの引数は一様な語になるので、`rest_references` と巻き戻しの手順2を簡単にする
- spec: vm.md の「`CodeBase` と `FnEntry`」「追記」「呼び出しと戻り」「シグネチャ」「変換のフレーム」「巻き戻し」「`RunStats`」、lir.md の「開く」「ダミーの値」、object-model.md の「クロージャ」(部分適用は一様な語を足す)、status.md の「実行の費用」
- 完了の条件:
  - `cargo test`
  - `cargo clippy --all-targets -- -D warnings`
  - `grep -rn "SourceSig\|source_ret\|RetBoxStruct\|conversions\|Receive\|Class" crates/eml_vm/src crates/eml_lir/src --include='*.rs'` が空になる
  - `cargo test -p eml_core_ir --test integration lowering::` が通る
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` が通る。`-Zmiri-tree-borrows` を足しても通る
- 種類: 1 と 2 (7.3 の T-8 の行)

### T-9: 測り直しと docs

- 目的: 段階を閉じる
- 作業:
  - きれいな作業ツリーで `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` を実行し、benchmarks.md の「今の数値」の3つの表を置き換える
  - benchmarks.md の「呼び出しの規約の引き換え」を、新しい引き換え (クロージャの引数の一様化と版) で書き直す
  - roadmap.md の「入口とシグネチャ」「結果の変換が要る末尾呼び出し」「ネイティブのコードを使い回すときの鍵」(鍵は名前と、開いた後の LIR と形と配置のハッシュ) を書き直す
  - testing.md の「LIR のテキストの形」、architecture.md の `sig.rs` への言及
  - lir.md の「末尾の位置の呼び出し」に D-13 の保証と例外を書く
  - lir.md の「LIR の命令」「構造の規則」R7、「位置の規則」「関数の ABI」(役割を `split` と一様な値の約束と `f$uniform` の形と振り替えの規則に)、「`apply` の場所」(消す。`_` の境目の説明は「関数の ABI」に移す)、「パスの順番」「box の挿入」「縮約」「verifier」、core-ir.md の「Core IR の命令」「型の Repr」「EP が足す変数の印」「複製」「鍵と名前」「handler の複製」「`con` の値の形についての特殊化」「EP が作る補助の関数」「yield の確認のない呼び出しの後」「継続の関数」のうち、T-5 から T-8 で書き終えていない所
- 完了の条件: `cargo test`、`cargo fmt --check`。docs と code から docs/plans を引いていない (`grep -rn "docs/plans" crates docs std` が空)

### T-10: LIR のパスのテストを eml_lir に移す (含めることに決まった)

- 決まったこと: 行わない。移す4つのファイルのテストはどれもソースからパイプラインを通すもので、testing.md の「テストの置き場所」は、それを `crates/eml_core_ir/tests` に置き、`eml_lir` の dev-dependency にフロントエンドを入れないと決めている。今の配置がこの規則どおりなので、利用者がやめると決めた

- 作業: `crates/eml_core_ir/tests/{boxing,contract,perceus,unwind}.rs` を `crates/eml_lir/tests` に移し、両方の `tests/main.rs` を直す
- 完了の条件: `cargo test` で、テストの数と結果が移す前と同じである
- 種類: 3

### T-11: (条件付き) `<g>{k}` の複製を既知の継続で特殊化する

- 条件: 関門 G で `scripts/csv2jsonl` の行だけが通らないとき
- 決まったこと: T-6 の後の関門 G では、csv2jsonl の実時間 (+6.2%) と LIR の文の総数 (+3.2%) の2つが通らなかった。利用者は、T-11 を入れて測り直すことに決めた
- 作業: handler に特殊化した `<g>{k}` の複製を、既知の継続のクロージャでさらに複製し、`resume k(v, s)` を直接の呼び出しにする (core-ir.md の「複製」と同じ規則)
- 完了の条件: 開発用の数え方で、csv2jsonl の箱を要る `apply` の場所が 0 になる。`bench/scripts.sh` の csv2jsonl の行が T-6 の前より遅くない

## 7. 実行の順番と確かめ方

### 7.1 順番

```
T-1 ─┐
T-2 ─┼─ T-3 ─ T-4 ─ T-5 ─ T-6 ─ T-7 (関門) ─ T-8 ─ T-9 ─ T-10
     │                          └ T-11 (条件付き、T-7 の前に入れて測り直す)
```

T-1 と T-2 は並べてよい。それ以降は1本の列である。

### 7.2 全体の確かめ方

- 各タスクの後に `cargo test` と `cargo clippy --all-targets -- -D warnings`
- T-4、T-6、T-8 の後に Miri の VM のテスト (2つの設定)
- T-7 で D-14 の測定をする。開発用の数え方は T-6 で入れたものを使う
- 段階を閉じる前に、bench の3つのスクリプトをきれいな作業ツリーで測る (T-9)

### 7.3 テストの変更 (この計画の承認が合意になる)

種類1は1つずつ、種類2は範囲で書く。

- T-2:
  - 種類2: `crates/eml_types/tests/scaling.rs` の4つの生成器に `<Panic>` を足す。生成したプログラムが E2002 になる古い入力を直すもので、合否の条件は変えない
- T-3:
  - 種類1: `crates/eml_lir/tests/verify.rs` の `v2_rejects_passing_unr_to_lin_at_each_boundary` を `v2_rejects_passing_lin_to_unr_at_each_boundary` に書き直す
  - 種類1: `crates/eml_lir/tests/contract.rs` の `a_multi_value_call_with_an_incompatible_mark_is_not_tail`、`crates/eml_lir/tests/verify.rs` の `v2_runs_only_on_jump_and_return_at_the_lowered_level`、`crates/eml_core_ir/tests/verify.rs` の `v2_runs_only_on_jump_and_return_at_the_core_ir_levels` の入力を `lin → unr` にする
  - 種類2: `crates/eml_lir/tests/text.rs` の `marks_have_a_join_a_compatibility_and_an_order` の値
- T-4:
  - 種類1: `tests/ui/check-fail/types/unr_value_in_once_field.em` を `tests/ui/run/kinds/` に移し、動かせる `main` を付ける
  - 種類1: `tests/ui/check-fail/linearity/parameter_called_twice_in_once_field.em` を `tests/ui/run/kinds/` に移す
  - 種類1: `crates/eml_types/tests/linearity.rs` の `an_unr_function_value_does_not_fit_a_once_field` と `a_parameter_called_twice_does_not_fit_a_once_field` を消す (移した UI テストが同じことを確かめる)
  - 種類1: `crates/eml_types/tests/effects.rs` の `a_clause_cannot_use_a_parameter_arrow_as_a_linear_one` を、渡してよいことを確かめるテストに逆にする
  - 種類2: `tests/ui/check-fail/linearity/once_continuation_through_effect_argument.em` の E3001 を E3002 にし、コメントを書き直す
  - 種類2: `crates/eml_types/tests/effects.rs` の `a_linear_arrow_is_displayed_as_once` と `a_linear_arrow_nested_in_a_tuple_is_displayed_as_once` は E3005 になる (正しい)。`once` を表示する E2001 が出るように入力を書き直す
  - 種類2: `crates/eml_core_ir/tests/translate.rs` の `partial_and_extra_arguments_use_closures` の `tobj poly` が `tobj` になる
  - 種類2: F で決まった未決の変数を含む診断の文言が `once Int -> Int` から `Int -> Int` になる所 (今の一式にはない)
  - 種類3: `tests/ui/run/kinds/references_in_once_positions.em` と `tests/ui/check-fail/linearity/top_level_function_through_effect_label.em` (E3002 のまま) のコメントと参照
  - 新しい run のテスト (`tests/ui/run/kinds/`): 下の「試しの中身」の c1、c3、c4、c6、c9、c10、c11_order_b、multi1、multi2、rv3_b、tuple1、op_once と、純粋な row を持つ再帰する局所関数
  - 新しい check-fail のテスト (同じ内容のテストがまだないとき): deep2 (E3002、深さの限界)、multi3 (E3006)、c5 (E3002)、n1 (E3001)
- T-5:
  - 種類2: `role ` を含むすべての Core IR と LIR のテキストのスナップショット
- T-6:
  - 種類1 (利用者が T-6 の実装中に認めた): D-3 が消す規則そのもの (型どおりの `apply` の場所、末尾の位置の例外、型どおりのクロージャ、手で書いた LIR で通る変換の道) を確かめるテストを、消すか新しい規則を確かめる形に書き直す。対象は T-6 のコミットで次に1つずつ書く。T-8 の一覧のうち T-6 で通らなくなるものは、ここに移す
    - `crates/eml_core_ir/tests/boxing.rs`: `no_boxed_wrapper_is_made` を `a_top_level_function_used_as_a_value_keeps_its_typed_abi` に書き直す (値の参照は `&inc$uniform` を指す)。`applies_of_a_typed_function_value_convert_nothing` を `an_apply_result_goes_to_the_next_apply_in_its_box` に、`a_body_ending_in_a_typed_call_makes_it_a_tail_call` を `a_body_ending_in_a_typed_call_tail_calls_its_uniform_variant` に書き直す (型どおりの `apply` と末尾の例外がなくなる)
    - `crates/eml_core_ir/tests/optimize.rs`: `a_con_copy_keeps_its_split_only_when_the_uniform_parameters_stay` を `a_con_copy_keeps_the_uniform_parameters_after_the_opened_ones` に書き直す (複製の `split` の規則が変わる)。`handler_specialization_functions_are_typed` の場所の検査を `apply` の数に替える。`a_clause_copy_applies_k_at_the_site_of_the_resume` を消す (`resume` の場所がなくなる)
    - `crates/eml_core_ir/tests/text.rs`: `a_site_repr_is_written_on_the_constants_of_a_typed_apply_only` を消す。`splits_and_sites_round_trip` から場所の検査を除く (場所の構文がなくなる)
    - `crates/eml_core_ir/tests/evidence.rs`: `ep_applies_are_uniform_sites` を `ep_calls_closures_through_apply` に書き直す (一様な場所の印がなくなる)
    - `crates/eml_core_ir/tests/verify.rs`: `a_site_entry_must_match_its_variable` と `a_resume_site_entry_must_match_its_variable` を消す (場所がなくなる)
    - `crates/eml_lir/tests/boxing.rs`: `a_typed_apply_converts_only_a_constant_to_its_entry` を `an_apply_boxes_constant_arguments_like_variables` に、`a_uniform_site_converts_its_arguments_and_result` を `an_apply_converts_its_arguments_and_result` に、`an_incompatible_tail_call_becomes_tail` を `an_incompatible_tail_position_call_is_not_a_tail_call` に書き直す。`closure_arguments_convert_to_the_callee_params` は `depth@[_]$uniform` を、`a_clause_call_uses_the_reprs_of_its_operation` は受け直した結果を期待する (型どおりの `apply` と末尾の例外がなくなる)
    - `crates/eml_lir/tests/contract.rs`: `an_incompatible_tail_position_call_becomes_tail` を `an_incompatible_tail_position_call_is_not_made_tail` に書き直す。`a_clause_call_returned_at_the_end_becomes_a_tail_call` と `a_multi_value_call_returned_in_order_becomes_tail` の、互換でない呼び出しを `tail` にしない期待値に替える (D-13)
    - `crates/eml_lir/tests/flatten.rs`: `a_box_from_a_tail_position_call_is_returned_as_one_value` を `a_box_from_a_tail_position_apply_is_opened_to_its_leaves` に書き直す。`calls_applies_and_returns_carry_leaves` の `apply` と `closure` に箱を渡す (末尾の例外と型どおりの `apply` がなくなる)
    - `crates/eml_lir/tests/text.rs`: `multi_value_forms_read_back` から場所の部分と構文の誤りを除き、`splits_and_sites_round_trip` から場所の検査を除く (場所の構文がなくなる)
    - `crates/eml_lir/tests/verify.rs`: `an_incompatible_return_is_accepted_only_after_a_tail_position_call` を `an_incompatible_return_is_rejected_even_after_a_tail_position_call` に、`a_function_value_need_not_be_uniform` を `a_function_value_is_uniform_beyond_its_captures` に、`an_incompatible_tail_call_result_is_accepted` を `a_tail_call_result_is_compatible_with_the_ret` に書き直す。`a_closure_checks_its_argument_scopes` と `v2_lets_unr_pass_to_lin_at_each_boundary` の入力を一様な関数の値にし、`source_records_open_to_the_leaves` を `apply` なしに書き直す。`the_contracted_level_rejects_an_incompatible_return_after_a_tail_position_call`、`an_incompatible_return_left_by_contract_is_reported_after_contract` (縮約の後の段がなくなる)、`a_site_entry_must_match_its_variable`、`a_source_result_is_a_struct_on_a_flattened_apply` (場所と `source_ret` がなくなる) を消す
    - `crates/eml_vm/tests/conversions.rs` (T-8 から移す): 型どおりの場所か変換のフレームを手で書いた LIR で通す次のテストを消す。`a_uniform_site_converts_into_and_out_of_a_typed_function`、`a_signature_too_long_to_pack_is_compared_by_its_reprs`、`a_boxed_argument_frees_a_unique_box`、`a_boxed_argument_decrefs_a_shared_box`、`a_typed_site_boxes_into_a_uniform_function`、`a_result_mismatch_pushes_one_conversion_frame`、`a_uniform_site_boxes_the_result_of_a_typed_function`、`tail_applies_compose_conversions`、`direct_tail_calls_compose_their_conversions`、`a_tail_clause_composes_its_conversion`、`a_composed_identity_pops_the_frame`、`a_direct_tail_call_converts_its_result`、`yielding_through_a_conversion_frame_writes_a_dummy`、`an_abort_unwinds_a_conversion_frame`、`ui_programs_take_the_conversion_paths_they_describe` (verifier が型どおりの場所と互換でない `tail` を拒むので、入力を書けない)
    - `crates/eml_vm/tests/conversions.rs` (T-8 から移す): `a_typed_apply_takes_the_fast_path` を `a_uniform_apply_takes_the_fast_path` に、`a_partial_application_by_the_vm_lays_out_raw_words` を `a_partial_application_by_the_vm_copies_uniform_words` に、`apply_rest_keeps_raw_words` を `apply_rest_releases_its_reference_arguments_on_unwinding` に書き直す。`a_closure_keeps_a_float_capture_raw` は箱の数を 2 にする (部分適用の引数が一様になる)。`a_constructor_value_applied_at_its_type_does_not_convert` を `a_constructor_value_applied_through_apply_does_not_convert` に名前を変える (型どおりの `apply` がなくなる)。`the_capture_count_is_packed_with_the_function`、`a_mismatch_between_different_types_is_internal`、`a_tail_call_between_reprs_of_different_types_is_not_compiled` は残す (verifier にかけない VM の見張りのまま通る)
    - `crates/eml_vm/tests/structs.rs`: `a_box_result_comes_back_as_leaves_through_a_conversion_frame`、`a_struct_beside_a_converted_scalar_keeps_its_leaves`、`tail_conversions_reach_struct_results` を消す (変換のフレームを通る入力を書けない)。構造体を `apply` に渡す次の5つのテストを、構造体を箱で渡す形に書き直す。`a_struct_result_crosses_a_conversion_frame` を `a_struct_result_crosses_an_apply_in_a_box` に、`struct_arguments_convert_to_and_from_boxes` を `struct_arguments_cross_an_apply_in_boxes` に、`a_one_leaf_struct_crosses_an_apply_in_one_word` を `a_one_leaf_struct_crosses_an_apply_in_a_box` に、`partial_application_captures_struct_leaves` を `partial_application_captures_a_struct_box` に、`an_abort_releases_the_reference_leaves_held_by_apply_rest` を `an_abort_releases_the_struct_box_held_by_apply_rest` にする
    - `crates/eml_vm/tests/miri.rs`: `a_boxed_argument_unboxes_into_a_typed_function` と `a_conversion_frame_round_trip` を消し、`typed_apply_with_raw_captures` を `an_apply_with_raw_captures` に書き直す (型どおりの場所がなくなる)
    - `crates/eml_vm/tests/scaling.rs`: `incompatible_tail_calls_keep_the_stack_flat` の `convert` と `conversions` の検査を、版への末尾呼び出しの検査に替える。`non_tail_conversions_count_in_the_stack` を `non_tail_applies_through_a_variant_count_in_the_stack` に書き直す (変換のフレームが版のフレームになる)
    - `crates/eml_vm/tests/codegen.rs`: `an_apply_carries_its_site_signature` を `an_apply_carries_a_uniform_signature` に書き直す (場所のシグネチャがなくなる)
    - `tests/ui/run-fail/panic/panic_through_conversion.em` と `tests/ui/run/unwind/abort_through_conversion.em` の頭のコメントは、変換のフレームが版になったので T-6 で書き直す。`tests/ui/run/functions/function_values_inside_data_called_generically.em` と `polymorphic_recursion_with_function_values.em` の頭のコメントも、実行時の変換を説明していたので書き直す。名前を変える移動は T-8 に残す
  - 種類2 (同じく認めた): eml_core_ir のテストの `split` の値、bench.rs の `unwind_points` のスナップショット
  - 種類2: bench.rs のすべての `RunStats` のスナップショット (`apply_float`、`apply_int`、`ep_float`、`trade_offs` と、`Float` や構造体を捕まえるクロージャを持つプログラムが変わる)
  - 種類2: `uniform apply`、`source(`、Repr の付いた `apply` の定数、`apply` の `-> X{…}` を含むすべての Core IR と LIR のテキストのスナップショット
  - 種類2: シグネチャの文字列を含む VM の逆アセンブルのスナップショット
  - 種類2: `apply` の実行時の変換や変換のフレームを説明する UI テストの頭のコメント (`function_value_made_inside_a_uniform_instance`、`float_bits_through_structs`、`lin_struct_through_uniform`、`float_closure_partial_application`、`unspecialized_map_passes_floats`、`swap_map_unspecialized`)
- T-11:
  - 種類2: `crates/eml_core_ir/tests/optimize.rs` の `handler_specialization_functions_are_typed` が見る節の写しは `<g>{k}{…}` になり、`k` への `apply` の代わりに継続の関数の直接の呼び出しを数える (この規則が `apply` を消す)。テストの名前と合否は変えない
  - 種類2: bench.rs の `unwind_points` のスナップショット (`scripts/csv2jsonl` が 106 から 109)
  - 種類2: `<g>{k}` の名前か、`k` への `apply` を含む Core IR と LIR のテキストのスナップショット
  - 新しいテスト (`crates/eml_core_ir/tests/optimize.rs`): `a_clause_copy_is_copied_for_each_continuation`、`a_clause_copy_drops_the_captures_instead_of_k`、`a_clause_copy_with_a_handle_is_not_copied_for_its_continuation`
  - `k` として渡す継続の関数 `f{H}$hN` に、D-12 の「値の参照が残らない値として生まれた関数は `split` を持たない」を当てる (別のコミット)。新しいテストは `a_continuation_called_only_by_its_clause_copy_is_typed`。種類1と種類2の変更はない
- T-8:
  - T-6 で通らなくなったものと、T-6 で消えた `Call::Apply::source_ret`、`uniform apply`、場所の Repr の構文に関わるものは、T-6 の行に移した。下の一覧のうち、まだ残るのは `a_mismatch_between_different_types_is_internal`、`a_tail_call_between_reprs_of_different_types_is_not_compiled`、`the_capture_count_is_packed_with_the_function`、`typed_benches_do_not_convert` と、2つの UI テストの移動である
  - 種類1: `crates/eml_vm/tests/conversions.rs` の `a_uniform_site_converts_into_and_out_of_a_typed_function`、`a_closure_keeps_a_float_capture_raw`、`a_partial_application_by_the_vm_lays_out_raw_words`、`apply_rest_keeps_raw_words`、`a_mismatch_between_different_types_is_internal`、`a_tail_call_between_reprs_of_different_types_is_not_compiled`、`a_signature_too_long_to_pack_is_compared_by_its_reprs`、`a_boxed_argument_frees_a_unique_box`、`a_boxed_argument_decrefs_a_shared_box`、`a_typed_site_boxes_into_a_uniform_function` を消す
  - 種類1: 同じファイルの `a_typed_apply_takes_the_fast_path` と `the_capture_count_is_packed_with_the_function` を、一様な ABI を確かめるテストに書き直す
  - 種類1: `crates/eml_vm/tests/bench.rs` の `typed_benches_do_not_convert` を消す (欄がなくなる)
  - 種類1 (動かすだけで、出力は変わらない): `tests/ui/run-fail/panic/panic_through_conversion.em` を `tests/ui/run-fail/panic/panic_through_uniform_variant.em` に、`tests/ui/run/unwind/abort_through_conversion.em` を `tests/ui/run/unwind/abort_through_variant_and_apply_rest.em` に移し、頭のコメントを書き直す
  - 種類2: すべての `RunStats` のスナップショット (`conversions` の欄がなくなる)、消した verifier の文言、`convert` を含む逆アセンブルのスナップショット
  - 利用者が T-8 の実装中に認めたもの:
    - 種類1: `crates/eml_lir/tests/verify.rs` の `source_records_open_to_the_leaves` を消す (消した `source` の検査だけを確かめていた)
    - 種類1: `crates/eml_lir/tests/text.rs` の `multi_value_forms_read_back` から `source(..)` を読み書きする部分を除く (構文がなくなる)
    - 種類2: VM のすべての逆アセンブルのスナップショット (`crates/eml_vm/tests/codegen.rs` と `roots.rs`) の命令の位置と飛び先が 12 ずつ下がる (`BUILTINS` が 14 から 2 になり、変換の命令列がなくなる)
    - 種類2: 移した `panic_through_uniform_variant.em` のスナップショットの誤りの位置の行が、新しいファイルの名前を指す (移すとパスが変わる)
    - 名前を変えるだけで、合否は変えない: `crates/eml_vm/tests/conversions.rs` を `apply.rs` に移す。同じファイルの `a_conversion_return_keeps_the_function_that_returned_last` を `an_error_at_finish_names_the_function_that_returned_last` にし、`leave` が `tobj` を返す形にする (変換のフレームがなくなり、確かめるのは誤りに付く関数の名前だけになる)。`codegen.rs` の `an_apply_carries_a_uniform_signature` を `an_apply_carries_only_its_callee_and_argument_slots` にする (`apply` がシグネチャを持たなくなる)
  - 注記: `FnEntry` の公開の欄は `{ name, start, end, arity, frame_size }` で、crate の中だけの欄に `params` (開いた後の引数の Repr) と `ret_rc` を残す。D-12 でクロージャは捕獲を Repr のとおり走査する語と生の語に分けて持つので、クロージャを作るときと入るときに語の並べ方を引くためと、`Finish` が結果を手放すかを決めるためである。T-8 の完了の条件の grep に残る `boxing.rs` の `conversions` と `place_field_conversions` (box の挿入の静的な変換)、`verify.rs` の `Receiver` (呼び出しの結果の受け手) は、消した仕組みとは関わりがない
- T-10:
  - 種類3: テストのファイルを移すだけ

### 7.4 試しの中身 (T-4 の新しいテスト)

調べたときに使った試しの要点である。テストにするときは、名前を内容が分かるものにする。ここでの `run_once : (once Int -> Int) -> Int`、`run_opt : Option (once Int -> Int) -> Int` とする。

- c1、c3: 局所の関数を呼んでから `once` の位置に渡す (`let g = fn x -> x + 1` の後に `g 1` と `run_once g`)
- c4: `var` が持つクロージャを `once` の位置に渡す (型だけを変えると debug で V2 が panic した形)
- c6: 引数 `f : Int -> Int` を呼んでから `once` の位置に渡す
- c9、c10: `->` のフィールドから取った関数を `once` のフィールドに入れる
- c11_order_b: 注釈のないラムダの引数を、先に `once` に渡してから呼ぶ (F が要る順番)
- multi1: `Unr` のクロージャを `multi` の操作をまたいで持ち、`once` の位置に渡す。出力は `["ab", "ab"]`
- multi2: `multi` の `k` を `once` の位置に渡し、もう一度再開する。出力は `[1, 2]`
- rv3_b: `let o = Some g` を `run_opt o` で2回使う
- tuple1: `(Int -> Int, Int)` の値を2回使い、要素を `once` の位置に渡す
- op_once: 操作の参照を `once` の位置に渡す。出力は 7
- deep2: `Option (Option (Int -> Int))` の値を2回使う。E3002 のまま
- multi3: File を捕まえたクロージャを `multi` の操作をまたいで持つ。E3006 のまま
- c5: `Lin` の値を捕まえたクロージャを2回使う。E3002 のまま
- n1: 再帰する局所関数が `Lin` の値を捕まえる。E3001 のまま

## 8. 危険と決めていないこと

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| U-1 | 関門 G の結果 | T-8、T-9 | T-7 で利用者が決める | 止めて T-6 を戻す。決まったこと: T-11 を入れた後の測り直しでは、LIR の文の総数 (+3.08%) だけが通らなかった。利用者はこれを受け入れて承認した。`handler_specialization_functions_are_typed` の書き換えも、利用者が種類2として認めた |
| U-2 | `f$uniform` の名前と `split` のテキストの形 | T-5、T-6 | 実装する人 | `<f>$uniform`、`uniform from N` |
| U-3 | 型どおりの引数の版 `f$ret` | T-6 | 関門 G の `Float`/構造体の累積の試しが通らないとき | 作らない (薄い形が自分を呼ぶ `Float` のループを型どおりに保つ) |
| U-4 | 節の入口と `tail clause` の例外 | T-9 | - | lir.md に例外として書く。入口の結果を一様にする案は追わない |
| U-5 | `FnEntry.arity` | T-8 | - | すべての関数で残す |
| U-6 | 置き換えの所で印を下げて `run_once g` を展開すること | 性能 | 測って要るとき | 印が等しいことを求めたまま |
| U-7 | 深い包摂 (2段目より深い位置、反変の位置) | 言語 | roadmap の見直す条件 | 入れない。E3002 のまま |
| U-8 | W と F の外での順番による違い | 型の検査 | - | 受け入れて types.md に書く。引数に型を書けば避けられる |
| U-9 | 構造体を translate で開かない判断の確度 (中) | D-2 | D-2 の見直す条件 | 開かない |

## 付録: レビューで変わったこと

同じ議論を繰り返さないための要点だけを書く。

- 呼び出し規約 (D-3):
  - 最初の案はすべての関数の値を薄い包みにする形で、末尾呼び出しの保証を失い、`eml run` の 2^27 語の上限に届くと指摘された。末尾の振り替えと写しの形を足して保証を戻した (D-13)
  - 次の案は値として使うすべての関数に版を作る形で、役割がしていたことを複製で作り直すと指摘された。値として生まれる関数をその場で一様にする `split` に分け、版を 71 個にした
  - 実測のない数値の上で古い仕組みを消すと指摘され、消す前に関門 G を置き、通らない条件を数値で決めた
  - 写しの形を決める規則が曖昧と指摘され、`needs_copy` を末尾の `apply`/`clause`/`resume` まで含めて定義した。自分を呼ぶ `Float` のループは薄い形で型どおりに残る
- データの配置 (D-5): 最初の案は構造体のフィールドへの埋め込みを今入れるものだった。得をするプログラムが `bench/` と `std/` にないと指摘され、数えて確かめたうえで見送り、見直す条件を数値で決めた
- 包摂 (D-7):
  - 最初の案は一番外側の矢印だけを広げ、操作を外していた。コンテナを2回使うと E3002 になる非対称を指摘され、共変の位置に1段だけ広げる形にした
  - 操作を外す理由は git の履歴になく、debug の検査が妨げていただけだったので、操作も含めた
  - 印の比べ方を向きのあるものにする所を、値の流れと置き換えで分けて監査し、置き換えは今のまま等しさを求める (D-11)
  - scaling の4つのテストが HEAD で失敗している原因を突き止め、直したうえで費用を測った (T-2)
- S1 の第2版は、D-3 の最初の案に合わせて「クロージャは一様な語だけを持つ」としていた。D-3 の最終の案では捕まえた値は型どおりのまま残るので、クロージャの物体の形は今のままとした (D-12)
- S1 は将来の配置を docs/spec の object-model.md と stdlib.md に書く案だった。docs/spec は今の状態だけを書く規則なので、roadmap に置いた (D-15)
