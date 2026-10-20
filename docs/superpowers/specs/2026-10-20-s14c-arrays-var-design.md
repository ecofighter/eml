# S14c `Array`、`MutArray` と局所的な可変変数の設計

## 目的と範囲

S14c の設計である。出発点は [ロードマップ](../../future/roadmap.md) の「S14 数値計算の整備」の `Array` と `MutArray` の項目と、S14 の設計 (`docs/superpowers/specs/2026-10-19-s14-numeric-design.md`) の S14c の行で、2026-10-20 の議論で範囲を広げた。S14 の設計のほかの節 (作業の分け方、S14a の `Panic`) は、そのまま S14c にも当てはまる。

S14c の目的は、S14d の計測 (エラトステネスのふるい、fannkuch-redux、spectral-norm など) を、読みやすい direct style で書けるようにすることである。ロードマップは `MutArray` を線形に受け渡す API (`write : MutArray a -> Int -> a -> MutArray a`) で使う形を考えていたが、その形ではループのたびに配列を引数と返り値で回すことになり、読みにくい。そこで、`MutArray` の上に、エフェクトで作る局所的な可変変数 `var` を入れる。`var` を使うループが、線形に受け渡す版と同じ速さで動くように、handler への特殊化も広げる。

目指す書き方は次のとおりである。

```
sieve : Int -> <Panic> Array Bool
sieve n =
  var marks = MutArray.make n True
  for 2 n (fn i ->
    if marks[i] then
      var j = i * i
      while (fn () -> j < n) (fn () ->
        marks[j] := False
        j := j + i))
  freeze marks
```

## 2026-10-20 に決めたこと

ユーザーが決めたことは次のとおりである。

- 配列の書き換えは、エフェクトを使った direct style で書けるようにする
- 局所的な可変変数 `var` を入れる。変数ごとに新しいエフェクトを作り、その handler が値を状態として持つ。同じ型の変数が何本あっても、内側の handler が外側を隠さない
- 末尾でない `perform` への handler の特殊化を S14c に含め、`var` を使うループを、線形に受け渡す版と同じ速さにする
- 変数の値がスタック (VM のフレームのスロット、ネイティブ化の後のレジスタ) に載ることは、最適化の普通の結果とし、言語として保証しない。特殊化が効かない形では、今と同じく状態を evidence (ヒープ) に置いて動かす
- 特殊化した関数は、結果と状態の組を返す。組の確保は S14e (小さな単一コンストラクタの型の構造体の表現) で消す。作業の順は S14c、S14d、S14e のままにし、組の確保の分も S14d と S14e の差として記録する

議論の中で私が案を出し、ユーザーが「よい」と答えたことは次のとおりである。

- `MutArray.make n x` と `MutArray.tabulate n f` で作り、`freeze` で `Array` にする。`Array` から戻す `thaw` は、一意ならそのまま渡し、共有されていれば写す
- `Array` は意味が不変で、`Array.set` は RC が 1 ならその場で書き換え、そうでなければ写す。ほかに `from_list`、`to_list`、`length` を置く
- 範囲外の添字は `Panic` にする。`Option` を返す版は入れない
- `Lin` の要素を持つ配列は S14c の範囲外にする。`MutArray` は extern が作る物体なので、`File` と同じく作るときに `lin` ビットを立てる。`poly` の物体の `lin` ビットの実行時の伝播は、マルチコア対応に残す
- `x[i]` は、名前の直後に空白なしで `[` が続くときだけ添字と読む。`f [i]` は今までどおりリストを渡す関数適用である。`Array` の読み出しにも `a[i]` を使える
- `:=` は予約記号にし、`var` は新しいキーワードにする
- `Lin` の変数の名前だけを書くと、値を取り出して変数を終える。`Unr` の変数では、ただ値を読む
- `for` と `while` は Prelude に置く
- S14c を3つの作業に分ける (下の「作業の分け方」)

## 目標、目標にしないこと、避けること

### 目標

- `Array` と `MutArray` が動き、範囲外の添字が `Panic` になる
- `var` で、スカラーと `MutArray` の変数を direct style で書き換えられる。配列を何本同時に使っても、書き方は変わらない
- `for` と `while` と局所関数で書いた `var` のループが、線形に受け渡す版と同じ仕事の回数 (`RunStats`) で動く。evidence を引かず、確保は線形に受け渡す版と同じである。ただし、S14e の前は、`Unit` 以外を返す関数の末尾でない呼び出しから戻る所で、組の確保が残る
- 特殊化の拡張は `var` に限らない。すべての節がすぐに再開する handler (`Std.State` など) なら、末尾でない `perform` を含むループも同じく特殊化される

### 目標にしないこと

- `Lin` の要素を持つ配列の API (要素を取り替える `replace` など)
- `Float` の要素を箱に入れずに持つ配列。要素は一様な語で持ち、`Float` の要素は箱に入る (下の「表現」)
- `MutArray` の分割と結合 (マルチコア対応)
- 配列のリテラルの構文 (`Array.from_list [1, 2, 3]` で書く)
- `var` をトップレベルや REPL の入力に書くこと。`var` はブロックの中の文である
- 末尾でない再帰の生産者の継続についての特殊化 (ロードマップの「再帰する生産者の継続についての特殊化」)。S14c が広げるのは、すべての節がすぐに再開する handler だけである

### 避けること

- `var` の変数を使う閉包が、変数の範囲の外で別の変数の handler に捕まること。再帰で同じ `var` の地点に入り直すと、静的な地点ごとのエフェクトでは起こりうる (下の「範囲の外へ出る閉包」)
- 範囲外の添字の `Panic` が、`x[i]` を書いた所の `try_panic` に届かないこと。`x[i]` の `Panic` は、書いた所の row に現れ、その場所から最も近い `try_panic` に届く
- 特殊化した版と EP の版で、意味 (結果、`Panic` の届く先、後始末) が変わること
- 線形に受け渡す今のプログラムの実行の仕事の回数が変わること (S14c-2 で `bench/` の期待値が変わるときは、下の「テストの変更」で扱う)

## 作業の分け方と完了の条件

| 作業 | 中身 | 完了の条件 |
|---|---|---|
| S14c-1 | `Array` と `MutArray` の核 | 下の「`Array` と `MutArray`」が動き、範囲外の添字が `try_panic` で捕まる。`MutArray` を線形に受け渡して書いたふるいが、検査モードで漏れなく動く |
| S14c-2 | すぐに再開する節だけの handler の、状態を渡す形の特殊化 | `Std.State` を末尾でない `put` で使うループが特殊化され、`evidence_steps` が 0 になる。手で状態を渡す形に書いた版と、確保、`box`、`unbox`、RC の回数が一致する |
| S14c-3 | `var`、`:=`、添字の構文、`for`、`while` | 上のふるいと、配列を3本使う fannkuch-redux の小さな版が `var` で書けて動く。どちらも `evidence_steps` が 0 で、`MutArray` を線形に受け渡して素直に手で書いた版と、確保の回数が一致する。S14e まで残る組の確保 (`Unit` 以外を返す経路の関数の、末尾でない呼び出しからの戻り) が入るなら、その分を数えて差として記録する |

どの作業も、いつもの検査 (`cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`cargo verus verify -p eml_rt`、`verusfmt --verus-only --check`、Miri の trusted のテスト) を通って終える。S14c-2 は S14c-1 と独立なので、先に進めてもよい。S14c-3 は両方の後である。

## S14c-1 `Array` と `MutArray`

### 型

- `Array a` と `MutArray a` は extern の型である。今の extern の型は型引数を持てない (E0011) ので、`extern data` に型引数を許す。extern のエフェクトに型引数を許さない理由 (row の中で重なったラベルを1つにまとめる規則) は、型には当てはまらない
- `Array a` の Kind は要素の Kind に従う。`MutArray a` の Kind はいつも `Lin` である。どちらも型引数について共変である
- extern の関数の引数に型変数を許す。今は、戻らない extern (`panic`、`exit`) の結果のほかに型変数を書けず、`crates/eml_core_ir/tests/externs.rs` がそれを確かめている。型変数の位置の Repr は `tobj` である
- extern の関数のシグネチャの型変数は `Unr` に固定する (私の判断。操作の引数の型変数を `Unr` に固定する規則と同じ考えで、本体のない宣言の使い方を Kind に推論できないためである)。そのため、`Lin` の要素の配列には、どの関数も使えない
- 型は Prelude に置く。添字の構文が `LangItems` から型と読み出しの関数を名指すためである。利用者向けの関数は `Std.Array` と `Std.MutArray` のモジュールに置き、`Array.length a` のように修飾して使う (私の判断。`Std.Float` と同じく、Prelude の名前を増やさない)

### API

`Std.Array` の関数は次のとおりである。

| 名前 | 型 | 意味 |
|---|---|---|
| `make` | `Int -> a -> <Panic> Array a` | 長さ n で、どの要素も x。n が負なら `Panic` |
| `tabulate` | `Int -> (Int -> <e> a) -> <Panic \| e> Array a` | i 番目を `f i` にする。添字の順に呼ぶ |
| `from_list` / `to_list` | `List a -> Array a` / `Array a -> List a` | リストとの変換 |
| `length` | `Array a -> Int` | 長さ |
| `get` | `Array a -> Int -> <Panic> a` | 読み出し。`a[i]` も同じ |
| `set` | `Array a -> Int -> a -> <Panic> Array a` | 1つを替えた配列。RC が 1 ならその場で書き換え、そうでなければ写す |
| `thaw` | `Array a -> MutArray a` | 一意ならそのまま、共有されていれば写して `MutArray` にする |

`Std.MutArray` の関数は次のとおりである。

| 名前 | 型 | 意味 |
|---|---|---|
| `make` | `Int -> a -> <Panic> MutArray a` | `Array.make` と同じ |
| `tabulate` | `Int -> (Int -> <e> a) -> <Panic \| e> MutArray a` | `Array.tabulate` と同じ |
| `length` | `MutArray a -> (Int, MutArray a)` | 長さ |
| `read` | `MutArray a -> Int -> <Panic> (a, MutArray a)` | 読み出し |
| `write` | `MutArray a -> Int -> a -> <Panic> MutArray a` | その場で書き換える |
| `freeze` | `MutArray a -> Array a` | `Array` にする。写さない |

- `read` と `length` は、配列を返すための組を確保しない (私の判断。数値計算の内側のループで、読み出しのたびに確保が起きないようにするためである)。extern は引数を消費するので、`read` を「組を作る extern」にすると、読み出しのたびに組を確保する。そこで、配列を借りて要素を読む命令と、その結果と配列を `con` で組にする形に translate で開く。組を受け取ってすぐ開く所 (`let (v, m) = MutArray.read m i` や、下の `var` の脱糖) では、融合の既知のコンストラクタで組が消える。借りて読む命令を、extern の行に借用の印を足して作るか、primitive にするかは計画で決める
- 範囲外の添字の `Panic` の文言は、`String` の位置の誤り (`IndexFault`) にならい、添字と長さを含める。文言は計画で決める
- `Panic` を起こす関数を `Panics` の行にし、`try_panic` で捕まえられるようにする。`length`、`freeze`、`thaw`、`from_list`、`to_list` は `Pure` である
- `var` の添字の脱糖 (下の S14c-3) は、`Panic` を起こさずに範囲外を知らせる読み書きを使う。たとえば、範囲内なら値、範囲外なら長さを返す関数である。これは Prelude の中だけの関数にする

### 表現

- `Array` と `MutArray` は同じ物体である。要素を走査する語として並べ、`Data` と同じ形 (255 語以上は拡張した数の語) を使う。要素の位置は一様な位置で、Repr は `tobj` である。`Int` は即値、`Float` は箱になる (私の判断。箱を持たない `Float` の配列は、型から決める呼び出しの規約と一緒に後で考える。Python のリストも `float` を箱で持つので、S14d の比べ方としては対等である)
- `MutArray` の物体は、作るときに `lin` ビットを立てる。`freeze` は一意な物体のビットをその場で下ろし、`thaw` は一意ならその場で立て、共有されていれば写した物体に立てる。`lin` ビットの立った物体を `dup` すると、検査モードが今までどおり `LinDup` で断る
- `Array.set` と `thaw` の写しは、要素を1つずつ `dup` して新しい物体に並べる
- `Data` の種類のまま使うか、同じ形の新しい種類を足すかは、計画で決める。新しい種類にすると、検査モードが `switch` や `unpack` での取り違えを `WrongKind` で断れるが、Verus の不変条件と回復の表に行が増える。どちらでも token にはしない ([オブジェクトモデル](../../spec/object-model.md) の「種類」の、`Array` は別の種類が要るという見通しを、この決定で書き直す)
- 回復は、`lin` ビットのない配列を `Data` と同じく写す。`MutArray` は `lin` ビットを持つので、今の規則どおり `Unrecoverable` になる
- 写しも書き換えも語の単位なので、信頼の一覧に行を足さずに Verus で確かめられる見込みである。長さの桁あふれは、今の `too_large` (T5) を使う

### 実行系

- VM は、`get`、`set`、`read`、`write`、`length` を専用の命令にする (私の判断。数値計算の内側のループで、`Op::Extern` の振り分けを通らないようにする)。誤りの道は、`Int` の算術と同じく `step_failed` から `Panic` に届ける
- そのほかの関数は `Op::Extern` にする
- `eml_extern` の行は、`tobj` の引数と結果、`Array` と `MutArray` の型の行 (型引数と Kind の規則) を持てるようにする

## S14c-2 状態を渡す形の特殊化

### 今の制限

S13e-2 の handler への特殊化 ([Core IR](../../spec/core-ir.md) の「handler への特殊化」) は、handle の本体から末尾の位置の直接の呼び出しでたどれる関数だけを経路にする。末尾でない `yields` の関数の直接の呼び出しが経路にあると、handle を EP の道に残す。そのため、次のようなループは特殊化されない。

```
loop i =
  if i < n then
    bump i          -- bump が perform する。末尾でない
    loop (i + 1)
  else ()
```

2026-10-20 の試作では、`Lin` の状態を持つ `State` に似たエフェクトで1000万回の更新をした。線形に受け渡す版は 0.21 秒 (命令数 74 億)、エフェクトの版は 1.12 秒 (命令数 286 億) だった。EP の道では、`perform` のたびに `ev_lookup`、節の呼び出し、状態の箱の出し入れ、yield の確認がかかる。

### 状態を渡す形

効果の操作がどれも再開する操作で、どの節も「すぐに再開する節」(Core IR の「handler の節の形」) である handler H を、新しい形で特殊化する。

- 経路は、handle の本体から、`yields` の関数の直接の呼び出しを、末尾の位置かどうかによらずたどって届く関数である
- 経路の関数 f の複製 `f{H}` は、f の引数に、節と `return` の節の捕獲と、状態つきなら状態を足して受ける。結果は、状態つきなら f の結果と次の状態の組、状態のない handler なら f の結果である。ただし、状態つきで f の結果の Repr が `unit` なら、次の状態だけを返す (私の判断。`for` の本体のような `Unit` を返す関数が、呼ばれるたびに組を確保しないためである)。`return` の節は複製の中で呼ばない
- handle の地点では、本体の複製を呼んで組を開き、`return` の節を結果と状態で呼ぶ
- 経路の関数への呼び出しは、末尾なら複製の末尾呼び出しにし、末尾でなければ複製を呼んで組を開き、次の状態で続ける
- H 宛ての `perform` には節の本体を展開する。末尾の `resume k(v, s)` は、`perform` の後ろの続きへのジャンプにし、v と s をブロックの引数で渡す。継続の関数 (`$hN`) と合流のブロックの関数 (`$hjJ`) は作らない。ブロックは前向きの辺だけで並べられ、コードは `perform` の数に比例して育つ
- 経路の関数に、`mask` の付いた呼び出し、入れ子の `handle`、`resume`、`yields` の `apply` があれば、今と同じく特殊化しない。入れ子の `handle` を断るのは、`try_panic` のような handler が経路の上に挟まると、展開した節の中の中断の届く先が EP の道と変わるためである
- 入れ子の handler は内側から特殊化する (私の判断)。`var` はどれも handle なので、ふるいでは `marks` の経路の上に `var j` の handle があり、fannkuch では3つの handle が重なる。内側の handle を先に特殊化すると、その handle は複製の呼び出しに変わって消えるので、外側の handler の経路は、内側が作った複製を通ってたどれるようになる。外側の handler は、内側の複製の複製 (`f{H1}{H2}` のように名前を重ねる) を作る。そこで、handlers の段を、経路にまだ特殊化していない handle を含まない handle から順に処理し、変わる handle がなくなるまで繰り返す。各回の複製は、その時点のコードから作るので、名前で中身が決まる規則は保たれる。特殊化しない handle (`try_panic` のような `never` の操作だけのエフェクトの handle を含む) が経路に残る外側の handler は、今と同じく特殊化しない。この段の作業は S14c-2 に含め、`Std.State` を2つ重ねたテストで確かめる
- 経路の関数がほかのエフェクトで yield するのは構わない。その関数の複製は `yields` のまま残り、状態はふつうの変数として EP の継続に捕まる
- 操作に `never` の操作を含む handler と、「それ以外の節」を持つ handler は、今の末尾の経路の形のまま扱う
- すぐに再開する節だけの handler は、経路がすべて末尾呼び出しでも新しい形にする (私の判断。同じ handler に2つの形を持たないためである。状態つきの handler では、handle の実行ごとに組を1つ確保するようになる)
- 複製の名前と、Core IR のテキストでの書き方は計画で決める。鍵に `return` の節の関数を含め、名前で中身が決まる今の規則を保つ

### 速さの見込み

- `for` と `while` は Prelude の高階関数なので、S13e-1 の既知の閉包の写しで本体の閉包が直接の呼び出しになり、経路に入る
- 組を確保するのは、経路の関数が末尾でない呼び出しから戻る所だけである。末尾再帰で回るループでは、ループの終わりに1回だけ確保する。S14e で組が構造体になれば、この確保も消える

## S14c-3 `var`、`:=`、添字、`for`、`while`

### 構文

- `var x = e` と `var x : T = e` は、ブロックの文である。`var` はキーワードになる。今 `var` を名前に使っているのは `std/Env.em` の非公開の extern だけなので、それを別の名前にする
- `x := e` と `x[i] := e` は、ブロックの文である。式としては書けない。`:=` は予約記号になる (今は `:` で始まるコンストラクタの演算子として字句に切られる)
- `e[i]` は後置の添字である。名前、`)`、`]` の直後に空白なしで `[` が続くときだけ添字と読み、関数適用より強く結び付く (`f xs[i]` は `f (xs[i])`、`a[i][j]` も書ける)。空白があれば今までどおりリストを渡す関数適用である
- 上の構文の文法と字句の規則は、[文法](../../spec/grammar.md) と [字句](../../spec/lexical.md) とレイアウトの規則に足す

### 意味

- `var x = e` の範囲は、同じブロックの、その文より後ろのすべてである。範囲の中のラムダと局所関数からも使える
- 変数ごとに、新しいエフェクト `L` を1つ作る。`L` は利用者が書けない名前のエフェクトで、診断では `var x` と表示する。範囲の後ろは、`L` の handler (状態は x の値) の本体になる。`x` を使う式と関数の row には `L` が推論される
- `Unr` の変数では、`x` は今の値を読み、`x := e` は値を替える
- `x[i]` は、`x` が変数なら、その `MutArray` の i 番目を読む。変数の型が `MutArray` でなければ型の誤り (E2001) である。`x` が変数でなければ、`Array.get x i` である。`x[i] := e` は、変数の `MutArray` の i 番目を書き換える。変数でない式への `:=` は誤りにする (新しい番号)
- 範囲外の添字は、`x[i]` と `x[i] := e` を書いた所で `Panic` を起こす。その `Panic` は書いた所の row に現れ、その場所から最も近い `try_panic` に届く
- `x[i] := e` は、i、e の順に評価してから書き換える
- `Lin` の値を持てる変数は、型が `MutArray` の変数だけにする (私の判断)。型が `Unr` でも `MutArray` でもない変数 (`File` や、Kind の決まらない型変数の値) は誤りにする
- `MutArray` の変数の名前だけを書けるのは、変数のブロックの最後の式の中で、ちょうど1回だけである (ラムダと局所関数の中は除く。私の判断)。そこで今の値を取り出し、変数の状態は空の配列 (確保しない不死の物体) に替わる。最後の式の中では、取り出しのほかにその変数を使えない。取り出さないまま終わる `MutArray` の変数と、それ以外の所で名前だけを書いた `MutArray` の変数は誤りにする。配列を何本も使ったブロックでは、最後の式でそれぞれを `freeze` するか `drop` する
- 最後の式も、ほかの文と同じく変数の handler の中で評価する。`Unr` の変数は、最後の式の中でも今の値を読む
- `MutArray` の変数への `x := e` は、古い値を捨てることになるので誤りにする。番号と文言は計画で決める
- handle の外側の row に `multi` の操作があると、`Lin` の変数は今の持ち越し規則 (E3006) で断られる

### 範囲の外へ出る閉包

`L` は `var` の地点ごとに1つで、再帰で同じ地点に入り直すと、同じ `L` の handler が重なる。そのため、外側の呼び出しで作った、x を使う閉包が、内側の呼び出しの handler に捕まることがありうる。これを型で断る。

- `L` が、範囲の外で束縛したものの型に現れたら誤りにする (新しい番号 E2019。文言は計画で決める)。範囲の外で束縛したものは、`var` を含むブロックの値、範囲の外の局所変数と引数、範囲の外で定義した局所関数、`var` を含むトップレベルの関数の型である
- 範囲の中で定義した局所関数は、自分を再帰で呼んでも `var` の地点に入り直さないので、型に `L` を持ってよい
- 調べるのは、その本体の型の推論と呼び出しの包含を解いた後である

### 脱糖

- `L` は、型引数に状態の型 s を持つエフェクトで、操作は `modify : (once s -> (a, s)) -> a` の1つである (私の判断。読み出し、書き換え、添字のどれも、この1つで書ける)。節は `fn modify f st -> f st`、`return` の節は値を返して状態を捨てる (`Unr` の変数の場合)
- `x` は `modify (fn s -> (s, s))`、`x := e` は e を先に評価して `modify (fn _ -> ((), v))` にする。`x[i]` と `x[i] := e` は、範囲外を値で知らせる Prelude の中だけの読み書きを `modify` に渡し、範囲外なら書いた所で `panic` を呼ぶ
- `MutArray` の変数の取り出しは、`modify (fn s -> (s, 空の配列))` にする。空の配列は、確保しない不死の物体を返す Prelude の中だけの関数で作る。`MutArray` の変数の handle の `return` の節は、残った状態 (取り出した後の空の配列) を `drop` する
- `L` は、HIR のどこかのモジュールに、名前を持たない合成の `effect` の項目として作る形と、ラベルの鍵を広げる形がある。前者なら、型検査、Core IR の効果の表、EP は今のまま使える。どちらにするかは計画で決める

### `for` と `while`

```
for : Int -> Int -> (Int -> <e> Unit) -> <Panic | e> Unit      -- lo 以上 hi 未満を順に
while : (Unit -> <e> Bool) -> (Unit -> <e> Unit) -> <e> Unit
```

- Prelude の eml のコードで書く。`for` の `<Panic>` は添字の足し算から来る

## 文書の更新

- [字句](../../spec/lexical.md)、[文法](../../spec/grammar.md)、[レイアウト規則](../../spec/layout.md): `var`、`:=`、後置の添字
- [式](../../spec/expressions.md): `var` の文、代入の文、添字、脱糖
- [宣言](../../spec/declarations.md): 型引数を持つ extern の型、extern の関数の型変数
- [型と Kind](../../spec/types.md): `Array` と `MutArray` の Kind、`var` のエフェクトと E2019
- [線形性](../../spec/linearity.md): `Lin` の変数の取り出しと代入の誤り
- [エフェクトと handler](../../spec/effects.md): `var` のエフェクト
- [標準ライブラリ](../../spec/stdlib.md): `Std.Array`、`Std.MutArray`、`for`、`while`
- [Core IR](../../spec/core-ir.md): 状態を渡す形の特殊化
- [LIR](../../spec/lir.md)、[VM](../../spec/vm.md)、[オブジェクトモデル](../../spec/object-model.md): 配列の物体、専用の命令、`lin` ビットの上げ下げ、回復
- [診断](../../spec/diagnostics.md): 新しい番号
- [ロードマップ](../../future/roadmap.md): S14 の作業の表で S14c を済みにし、`Array` と `MutArray` の論点の答えを書く。「言語機能と構文」の「局所的な可変変数の糖衣構文」(前提は可変参照) を、S14c でエフェクトで入れたものとして消す。[マルチコア対応](../../future/multicore.md) と [標準ライブラリへの申し送り](../../future/stdlib.md) の `MutArray` の記述も直す
- [計測の基準](../../implementation/benchmarks.md): S14c の節
- CLAUDE.md
- [実装の現在地](../../implementation/status.md) と [コンパイラの構成](../../implementation/architecture.md) は、S14 の終わりに直す

## テストの変更

### 種類1 (通過と失敗の変更)

- 見込みはない。今のテストとソースには、`var` を名前にしたもの (`std/Env.em` の非公開の extern を除く)、`:=`、空白なしの `x[` はない (2026-10-20 に `.em` と `.repl` を調べた)。Rust のテストの中に書いたソースに当たりがあれば、計画で1つずつ挙げる

### 種類2 (期待値の変更)

- S14c-2 で、すぐに再開する節だけの状態つきの handler は、末尾の経路でも新しい形になる。そのため、`crates/eml_vm/tests/bench.rs` の `RunStats` の期待値 (確保の回数など) と、`crates/eml_core_ir/tests` の特殊化の IR のスナップショットが変わる。特殊化の自動と手書きの組 (`specialization_matches_hand_written_work`、`specialization_matches_hand_written_rc`) の手書きの側も、新しい形に合わせて書き直す。範囲は「すぐに再開する節だけの handler を特殊化するテストのすべて」である
- extern の行と std のシグネチャを結ぶテスト (`crates/eml_core_ir/tests/externs.rs` の型変数の確かめ、`crates/eml_hir/tests/externs.rs`) を、型変数の引数と型引数を持つ extern の型を許す形に緩める
- `extern data` の型引数の E0011 を確かめるテストを、型引数を許す形に変える
- `crates/eml_cli/tests/snapshots/integration__ui__check_fail@kinds__extern_kind.em.snap` の E0011 の文面を、"cannot have parameters, `=` or `where`" から "cannot have `:`, `=` or `where`" に変え、ラベルも "only the name and its type parameters" に変える。`extern data` が型引数を受け付けるようになったためである
- `std/Env.em` の extern の改名で、名前を含む期待値が変わる所

### 足すテスト

- `Array` と `MutArray` の各関数、範囲外の `Panic` と `try_panic`、`freeze` と `thaw` の写す場合と写さない場合、検査モードの漏れと `LinDup` (`run`、`run-fail`、`eml_rt` の単体テスト)
- 状態を渡す形の特殊化の IR のスナップショットと、末尾でない `perform` のループの `RunStats`
- `var` のスカラーと `MutArray`、配列を3本使う計算、ラムダと局所関数からの使用、`Lin` の変数の取り出し (`run`)
- E2019、`MutArray` の変数への代入と取り出し忘れ、最後の式の外での取り出し、`Unr` でも `MutArray` でもない変数、変数でない式への `:=`、`MutArray` でない変数の添字、`multi` の操作をまたぐ `Lin` の変数 (`check-fail`)
- `x[i]` の `Panic` が、書いた所の `try_panic` に届くこと (`run`)
- `crates/eml_vm/tests/bench.rs` に、ふるいと fannkuch-redux の小さな版を、`var` の版と線形に受け渡して手で書いた版の組で足し、確保と `evidence_steps` が一致することを確かめる
- 字句と構文: `x[i]` と `x [i]` の違い、`a[i][j]`、`f xs[i]`、`:=`

## 作る人に任せること

- 配列の物体を `Data` の種類のまま使うか、新しい種類にするか
- 範囲外の添字の文言と、新しい診断の番号と文言
- `MutArray` の変数の取り出しで状態に置く空の配列を、不死の物体としてどこに置くか (凍結した領域など)
- `var` のエフェクトを合成の `effect` の項目にするか、ラベルの鍵を広げるか
- 状態を渡す形の複製の名前と IR のテキストでの書き方
- 専用の命令の番号の付け方と、`eml_extern` の行の型引数の持ち方
