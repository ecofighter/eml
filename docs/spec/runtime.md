# スケジューラ

位置づけ: 規範。

## 範囲

この文書は、`eml_sched`のスケジューラを定める。今は、このスケジューラを動かすエンジンはない。テストは、項がRustの閉包である`Exec`、`Script`を通してこれを動かす。VMは自分のスケジューラを動かし、それは[VM](vm.md)が定める。

スケジューラの判断は`eml_steps`の関数で、Verusで証明してある ([アーキテクチャ](../implementation/architecture.md)の「`eml_steps`の内部」)。この文書は、スケジューラがそれをどう行うかを述べる。今のrunの手順は、合わせた席で1つのworker、worker 0を動かし、ループはタスクのないrunの手順を踏む。各節は、今あるものを述べる。

## もの

| もの | 定義 | workerの間を移るか |
|---|---|---|
| worker | OSのスレッド1つ、`Ctx`1つ、ヒープ1つ、受け箱1つ、準備のできた列1つ、開いた一覧1つ、本物の時計ではI/Oのdriver1つ。worker 0は`main`を動かし、時計、判定、ENDを持つ。メインのworkerは呼ぶスレッドの上の追加のworkerで、取り替えられる`Park`を持つ。workerの番号`k`は、その欄と止める語を指す。論理的なworker IDではない | - |
| タスク | `Async`のタスク。登録表のセルと、待つ間はその継続である | 置いた後は移らない |
| 根のタスク | `main`の行が`Async`を持つとき、`main`を動かすタスク | 移らない |
| 仕事 | まだ始めていない`Job { g: Marked, id, site, done }`。開いた一覧の中にだけある | 始まるまで。取られるだけで、送られない |
| 子 | 始めた仕事か、親のフレームの中で動く`par`の子 (左の子はいつも、右の子は誰も取らなかったとき) | 移らない |
| 項 | `Exec::enter`が終わりか次の待ちまで動かすもの。`main` (`Item::Root(Root::Main)`)、タスクのエントリ、取った仕事である | 移らない |

今のコードにあるのは、workerと項`main`である。ほかの行は、そのコードを入れるときにその節が定める。

スケジューラ自身の型は次のとおりである。

- `WorkerCore`は、workerが自分で持つスケジューラの部分で、workerの番号と方針を持つ。`Send`ではなく、workerのスレッドだけが持つ。`Env`がfactoryに渡し、`Exec::with`が項の間に貸す。公開の作り方はない
- `Exec`はエンジンの、1つのworkerの項を動かすものである。`Workers`はrunのworkerのfactoryである (「runの手順」)
- `Halt`は、workerが止まる理由を表す (「runの手順」)

## workerのループ

`drive`はworkerのループで、`eml_steps::plan::next`のただ1つのインタプリタである。繰り返しのたびに事実 (`plan::Facts`) を集め、`next`に手順を尋ね、それを行う。`Exec::enter`を呼ぶのは`drive`だけで、`drive`を呼ぶのは`sequence::run`だけである。ループは手順を行う前に、それを`Point::Kernel(Label::Loop(step))`として方針に知らせる (「フック」)。

ループが今踏む手順は次のとおりである。

- `Stop`: workerの止める語に止めるビット (`INTERRUPT`、`TERMINATE`、`MEMORY`、`END`のどれか。`NOTICE`は含まない) が立っている。止まりがまだ記録されていなければ、ループはこの手順のために読んだ語で、`INTERRUPT`、`TERMINATE`、`MEMORY`のうちこの順で最初に立っているものの`RunFault::Stopped`を記録し、戻る。ENDだけでは何も記録しない。ENDは理由にならない。立てた者が理由を記録したか、runが成功したかのどちらかだからである
- `Root(Main)`: worker 0が`Item::Root(Root::Main)`に1回だけ入る。`enter`が`Err(halt)`を返すと、ループはそれを記録し (`Halt::Stopped`は`Cause::Halt`、`Halt::Panicked`は`Cause::Fault(Internal("a worker thread panicked"))`として)、同じ`halt`を返す
- `Return`: 根が終わり、worker 0のループが`Ok(())`を返す

ループがまだ踏まない手順は、`Internal("the loop reached a step it does not take")`として記録する。ループが今集める事実からは、そのような手順は出ない。

`enter`をまたいで、ループは`Exec`のポインタから導いたものを何も持たない。核を読むのは項の間に`Exec::with`を通してだけである (「信頼の一覧」のSM-5)。

## runの手順

`sequence::run`は、runのただ1つの手順である。`drive`を呼ぶのはこれだけで、ただ1つの`Control::clear_end`を持つ。エンジンは、自分のworkerのfactory (`Workers`) と`Start`を渡してこれを呼ぶ。`Start`は、runのすべての`Ctx`の所有者の表、host、`RunConfig`で、`RunConfig`の`control`がrunの`Control`である。今は合わせた席で1つのworkerを動かし、呼ぶスレッドがworker 0である。`Control`のworkerの数がほかの数のrunと、席が分かれたrunは、何も始めず、`Internal("this run sequence runs one worker in the merged seat")`を返す。

1. `Control::begin_run`がrunの終わりの欄を開く
2. runは呼ぶスレッドで、`Workers::policy`が返す方針でworker 0の`WorkerCore`を作り、runのcallbackと`Env`を渡して`Workers::on_worker`を呼ぶ。`Env`は核とrunの共有の部分を渡す。factoryは、runの所有者の表の上に`Ctx::for_worker`で`Ctx`を作り、自分の`Exec`を組み、それへのポインタでcallbackを1回呼び、核をそのスレッドで落とし、`Ctx`を切り離して持つ`Retired`を返す
3. callbackがループを動かす。ループが戻ったときが、理由によらず、runを終わらせる出来事である。panicを記録し、`Control::end_run`がすべての止める語にENDを立てる。記録は自分のcatchの下で動き、ENDはその後で立てるので、記録がpanicしてもENDは立つ。callbackを呼ばずに戻ったfactoryも同じ出来事で、`Internal("a worker left without running")`として記録する
4. `finish`が、呼ぶスレッドでrunを確定させる。最初に記録した止まりが勝ち、終了コードは`Control::exit_code`である。その後で`Control::clear_end`がENDを下ろす。確定がpanicしてもENDは下りる。確定は自分のcatchの下で動き、`clear_end`はcatchが何を返してもその後で動くからである。`finish`は、呼ぶスレッドのcallbackの終わりで動くか、そのcallbackが動かなかったときは、factoryが戻った後に`sequence::run`から動く
5. `sequence::run`は`Outcome { stop, exit, retired }`を返す。`retired`は、すべてのworkerの`Retired`をその場所とともに持つ

記録するのは`Stopped { worker, cause }`である。`cause`は、エンジンがそのworkerの`Retired`に誤りを持つときは`Cause::Halt`、スケジューラ自身の理由のときは`Cause::Fault(RunFault)`である。最初に書いたものが勝つ。

### panic

ほかのworkerがまだ動きうる間は、どの巻き戻しも`Ctx`もヒープも落とさない。ほかのスレッドが共有の物体を持っているヒープは落としてはならないからである ([オブジェクトモデル](object-model.md)の「`share`」)。そのため次のようにする。

- `Halt`は、受け取る側が何をしてよいかで定める。`Stopped`は、workerが止まり、そのヒープが操作の間にあることを表す。`Panicked`は、workerが止まり、そのヒープが操作の途中にあるかもしれないことを表す。受け取る側はそのヒープを切り離し、その中の何もたどらず、確かめない。`Exec::enter`はどちらを返してもよく、エントリがRustのpanicを捕まえたエンジンは`Panicked`を返す。ループはそれを変えずに渡す。3つ目の変種はない
- runのcallbackは、すべての文がcatchの下にある1つの関数である。ループ、ループの終わり、`finish`はそれぞれ自分のcatchの下で動くので、そのどれかのpanicは`Internal("a worker thread panicked")`として記録され、後のものもそのまま動く。そのときcallbackは`Err(Halt::Panicked)`をfactoryに返す
- `Err(Halt::Panicked)`のとき、factoryは`Ctx`を切り離して包むだけで、それでも`Retired`を返す。`Outcome.retired`はすべてのworkerを持ち、panicしたworkerのヒープをどうするかは、runの後に呼ぶ側が決める
- `Ctx`を切り離すとは、それをどのスレッドのエンジンの状態からも外し、落とさずに`Retired`に入れることである。切り離した`Ctx`を落とすか、たどるか、回復するかは、runの後に呼ぶ側が決める。エンジンのエントリがヒープの操作の途中でpanicしうるときは、`Halt`が`Panicked`だったworkerについて、呼ぶ側はそのどれもしない。`Scripts`の項は閉包なので、`Halt::Panicked`の後のヒープが操作の途中にあるのは、閉包が操作の途中でpanicしたときだけである。そうしない閉包のテストは、戻ってきた`Ctx`を落としてよい
- runが持つ`Retired`はどれも、それを受け取る式から`Outcome`に移すまで`ManuallyDrop`の中にあり、その間の手順はどれもcatchの下で動く。だからそのようなフレームの巻き戻しは、ヒープを落とさずにリークする
- factoryはcallbackの外でpanicせず、フレームの巻き戻しが`Ctx`を落とさずにリークするように`Ctx`を持つ。それでもフレームが巻き戻ったら、`sequence::run`はスレッドの根元で巻き戻しを捕まえ、`eml: a worker's set-up panicked outside its loop; aborting`をstderrに書き、プロセスを中止する。巻き戻しは`Ctx`をリークしたので、中止の前にほかのスレッドが解放した記憶を読むことはなく、中止は定義された終わりである。同じくcallbackの外で動く`Workers::policy`のpanicも、同じように中止する
- 捕まえたpanicのpayloadは、自分のcatchの下で落とす。その落とし方の中のpanicのpayloadは忘れる。どのpanicも`sequence::run`の外に出ない

## lockの順位

スケジューラのlockはどれも`rank.rs`を通して、`PoisonError::into_inner`で取る。lockの下でpanicしたスレッドは、状態を1回の代入が残すとおりに残したからである。順位は2つある。

| 順位 | lock |
|---|---|
| 1 | 登録表 (`cells::Shared`) |
| 2 | スケジューラのほかのすべてのlock。今は止まりの記録と、runの確定した終わりである |

順位2のlockは、ほかの順位2のlockを持たないときにだけ取り、順位2のlockを持つ間は登録表を取らない。だからスレッドが持つのは、多くても登録表と、その下の1つの順位2のlockである。登録表を中でlockする`cells`の関数を呼ぶことも、登録表を取ることに数える。デバッグのビルドでは、スレッドローカルな値がスレッドの持つlockを記録し、表明が違反を止める。同じ表明が、`Policy::at`をlockを持たずに呼ぶことを確かめる。今のスケジューラは登録表を取らないので、表明が見るのは順位2のlockとフックだけである。リリースのビルドは何も記録しない。`Mail`と`HostBin`のlockは`cells`の中にあって何も呼び返さないので、順位の外にある。

## フック

`Policy::at(&self, worker, point)`は、その地点を行うworkerが、行う前に、lockを持たずに呼ぶ。`Point::Kernel(label)`は`eml_steps`のカーネルの手順か効果 (`eml_steps::Label`) で、それを行うインタプリタの頭にある1つの文が知らせる。`Point::Shell(point)`は、どのカーネルも決めない、シェルの4つの地点の1つである。`JoinWait` (`wait_join`が待つ前)、`Submit` (hostが操作を自分のdriverに出す前)、`HandBack` (hostが完了を持ち主に渡すために`cells`を呼ぶ前)、`Leave` (workerの最後の`wake_all`の前) である。今はループが自分の手順を知らせ、シェルの地点はまだ知らせない。

方針は、スケジューラが読むものを何も決めない。テストは、`at`の中でworkerを止めて、交互の順を強いる。`Sched::Normal`と`Sched::Sequential`の方針である`InOrder`は、何も見ない。

## 信頼の一覧

Verusが`eml_rt`と`eml_steps`について証明することのほかに、スケジューラの記憶の安全が拠りどころにするもの。スケジューラ自身のファイルの`unsafe`のブロックはどれも、拠りどころにする行か約束を名指す。`doorbell.rs`のブロックはSM-8に、`cells`のブロックはそのmoduleの「呼ぶ側の約束」に拠る。

A群、シェルによるカーネルの実行。

- ST-1: カーネルが返す効果と手順はどれも、1回のアトミックな操作か1つのlockの区間として行い、返す事実はその区間の中で読む
- ST-2: カーネルのモデルは逐次一貫性を前提にする。`sleepers`、`searching`、`running`、`owed`は`SeqCst`である。走査は一覧を、その欄のlockの下でだけ読む。`listed`をlockなしで読むのは判定だけで、`running`の後である。ST-2が名指す欄は、それを入れるコードとともに来て、この行はそのコードを縛る
- ST-3: `std::sync::Mutex`、`Condvar`、スレッドのparkは、文書のとおりに動く

B群、スケジューラの境界の約束。

- SM-5、`Exec`の約束: `with`は、このworkerのスレッドの核と`Ctx`を自分の閉包にだけ貸し、閉包が戻った後はどちらの借用も持たない。`enter`をまたいで、呼ぶ側は`Exec`のポインタから導いたものを何も持たない。`enter`はその`Ctx`の上で項を1つ動かし、panicを捕まえたときは`Halt::Panicked`を返す
- SM-8、`doorbell.rs`の`libc`の呼び出し: doorbellが持つ2つのfdへの`pipe`、`fcntl`、`write`、`poll`、`read`、`close`と、OSの場所を返す関数を通した`errno`である
- SM-9、`Workers`の約束: `on_worker`は、workerの自分のスレッドで1回だけ動く。callbackを呼ぶ前に、そのスレッドで、runの所有者の表の上に`Ctx::for_worker`で`Ctx`を作る。callbackは多くても1回呼ぶ。callbackの外でpanicせず (`Workers::policy`も)、巻き戻しが`Ctx`をリークするように持つ。核をそのスレッドで落とし、`Ctx`を切り離して落とさずに`Retired`に入れて返す
