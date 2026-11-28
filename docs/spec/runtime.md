# スケジューラ

位置づけ: 規範。

## 範囲

この文書は、`eml_sched`のスケジューラを定める。今は、このスケジューラを動かすエンジンはない。テストは、項がRustの閉包である`Exec`、`Script`を通してこれを動かす。VMは自分のスケジューラを動かし、それは[VM](vm.md)が定める。

スケジューラの判断は`eml_steps`の関数で、Verusで証明してある ([アーキテクチャ](../implementation/architecture.md)の「`eml_steps`の内部」)。この文書は、スケジューラがそれをどう行うかを述べる。今のrunの手順は、合わせた席で1つのworker、worker 0を動かし、ループはそのworkerの上で`main`と根のスコープとそのタスクを動かす。各節は、今あるものを述べる。

## もの

| もの | 定義 | workerの間を移るか |
|---|---|---|
| worker | OSのスレッド1つ、`Ctx`1つ、ヒープ1つ、受け箱1つ、準備のできた列1つ、開いた一覧1つ、本物の時計ではI/Oのdriver1つ。worker 0は`main`を動かし、時計、判定、ENDを持つ。メインのworkerは呼ぶスレッドの上の追加のworkerで、取り替えられる`Park`を持つ。workerの番号`k`は、その欄と止める語を指す。論理的なworker IDではない | - |
| タスク | `Async`のタスク。登録表のセルと、待つ間はその継続である | 置いた後は移らない |
| 根のタスク | `main`の行が`Async`を持つとき、`main`を動かすタスク | 移らない |
| 仕事 | まだ始めていない`Job { g: Marked, id, site, done }`。開いた一覧の中にだけある | 始まるまで。取られるだけで、送られない |
| 子 | 始めた仕事か、親のフレームの中で動く`par`の子 (左の子はいつも、右の子は誰も取らなかったとき) | 移らない |
| 項 | `Exec::enter`が終わりか次の待ちまで動かすもの。`main` (`Item::Root(Root::Main)`)、タスクのエントリ (`Item::Entry`)、結果の項 (`Item::Root(Root::Result)`)、取った仕事である | 移らない |

今のコードにあるのは、worker、タスク、項 (`main`、タスクのエントリ、結果の項) である。ほかの行は、そのコードを入れるときにその節が定める。

スケジューラ自身の型は次のとおりである。

- `WorkerCore`は、workerが自分で持つスケジューラの部分で、workerの番号、方針、準備のできた列、`main`が置いた根のスコープを持つ。`Send`ではなく、workerのスレッドだけが持つ。`Env`がfactoryに渡し、`Exec::with`が項の間に貸す。公開の作り方はない
- `Entry`はタスクの項で、タスクのセル、タスクの論理的なID、始め方 (`EntryKind`。始める、再開する、操作の後に再開する)、項が参照を1つ持つ語 (閉包か継続) を持つ
- `Signs`は、各workerのフックの値を持つ (「フック」)
- `Exec`はエンジンの、1つのworkerの項を動かすものである。`Workers`はrunのworkerのfactoryである (「runの手順」)
- `Halt`は、workerが止まる理由を表す (「runの手順」)

## workerのループ

`drive`はworkerのループで、`eml_steps::plan::next`のただ1つのインタプリタである。繰り返しのたびに事実 (`plan::Facts`) を集め、`next`に手順を尋ね、それを行う。`Exec::enter`を呼ぶのは`drive`だけで、`drive`を呼ぶのは`sequence::run`だけである。ループは手順を行う前に、それを`Point::Kernel(Label::Loop(step))`として方針に知らせる (「フック」)。

ループは次の事実を自分で持つ。根の状態 (「根」)、今の準備のできた列の通りに残る項の数、最後の`Inbox`の手順かループの始めの後に、それぞれの源 (切り離した仕事、ほかのworkerの仕事、driver) を見たかである。ループが`next`に渡す事実はどれも、ループが読んだものか、ループが行った手順が書いたものである。

workerはループの始めに、自分のIDを1回だけ読む。ヒープの論理的なworker IDで、新しいヒープでは0、貸した`Ctx`ではセッションの今のIDである。workerは項の間をこのIDで過ごす。

ループの手順は次のとおりである。

- `Stop`: workerの止める語に止めるビット (`INTERRUPT`、`TERMINATE`、`MEMORY`、`END`のどれか。`NOTICE`は含まない) が立っているか、このworkerが止まりを記録した。項の`Halt`はすぐにループを出る。ループが続く間にこのworkerが記録したスケジューラの誤りは、次の手順であるこの手順でループを出る。止まりがまだ記録されていなければ、ループはこの手順のために読んだ語で、`INTERRUPT`、`TERMINATE`、`MEMORY`のうちこの順で最初に立っているものの`RunFault::Stopped`を記録し、戻る。ENDだけでは何も記録しない。ENDは理由にならない。立てた者が理由を記録したか、runが成功したかのどちらかだからである
- `Root(Main)`: worker 0が`Item::Root(Root::Main)`に1回だけ入る。`main`が`WorkerCore::set_root`で根のスコープを置いていれば根は開き、置いていなければループは`main`の後で戻る。`enter`が`Err(halt)`を返すと、ループはそれを記録し (`Halt::Stopped`は`Cause::Halt`、`Halt::Panicked`は`Cause::Fault(Internal("a worker thread panicked"))`として)、同じ`halt`を返す。どの項でも同じである
- `Root(Result)`: 判定が根のスコープの閉じたことを読んだ後、worker 0が`Item::Root(Root::Result)`に1回だけ入り、その後でループは戻る
- `Inbox`: 通りが終わり、源を見た後で、mailに項があるか、知らせを集めるよう求められているか、準備のできた列が空でない。workerは`NOTICE`を下ろし、自分が取ってよいmailの項を処理し (`cells::drain`)、それが再開するタスクを準備のできた列に積み、そのときの列の項の数だけを動かす通りを始める。この手順は、源を見たことの3つの事実を消す
- `Ready`: 通りに項が残っている。workerは準備のできた列の頭の項を取り、動かす。通りの間に積んだ項は、次の通りで動く
- `Detached`、`Foreign`、`Look`: workerは自分の切り離した仕事、ほかのworkerの仕事 (準備のできた列が空のときだけ)、自分のdriverを見る。開いた一覧も、ほかのworkerも、driverもない間は取るものがなく、どの手順も源を見たことを記録するだけである
- `Idle`: 準備のできた項がなく、mailは空で、知らせを集めるよう求められておらず、どの源も見た。worker 0は判定を動かす (「判定」)
- `Return`: 根が終わったか、`main`が根のスコープを置かず、worker 0のループが`Ok(())`を返す

項を開くときは、どのビルドでも、`evv`が`()`であること、yieldの途中でないこと、置いた失敗がないことを確かめる。速いモードの`switch_task`は、`Ctx`の持つ語を新しいIDが持つことを求め、これらがそれを満たすからである。その後でループは、項を`current`に書き、phaseに`Run`を書き、`switch_task`で項のIDに切り替える。タスクのエントリはタスクのID (`Entry.worker`) の下で、根の項はworkerのIDの下で動く。`Ok`を返した項を閉じるときは、`evv`とyieldについて同じ2つを確かめ、`current`に`NO_CELL`を書き、workerのIDに戻り、ほかのスレッドから受けた解放を取り込む (`reclaim_if_due`)。`Halt`を返した項は閉じない。`Halt::Stopped`の後はヒープが操作の間にあるので、ループは`switch_task((), ID, 0)`でworkerのIDに戻り、戻ってくる語、つまり止まった項の`evv`を放さずに捨てる。タスクのIDの節をworkerのIDの下で放すと検査モードでは`WrongOwner`になり、止まったrunのヒープは回復が取り替えるか落とすからである。したがって、panicしなかったrunの後、workerの`Ctx`はループの始めのIDの下にあり、新しいヒープに古いヒープのworker IDを与える回復は、セッションのIDを保つ。`Halt::Panicked`の後は、ループは`Ctx`にもヒープにも触れない。

準備のできた列は、積んだ順にエントリを持つ。`WorkerCore::schedule`は、`cells`の`Runnable`を`Entry`にする。`Runnable::Token`はtokenによる待ちで、スタックを持ったVMのエントリだけが作るので、`Internal("a token wait reached the native scheduler")`として記録する。スケジューラが渡す`depth_offset`の引数はどれも`0`である。エントリは語の参照を持ち、動かなかったエントリは語に触れずに`WorkerCore`と一緒に落とす。

`enter`をまたいで、ループは`Exec`のポインタから導いたものを何も持たない。核を読むのは項の間に`Exec::with`を通してだけである (「信頼の一覧」のSM-5)。

## 根

`main`の行が`Async`を持つとき、`main`の項は根のスコープを開き、根のタスクをそこに置き、そのスコープを`WorkerCore::set_root`に渡す。ループは`main`が戻ったときに根のスコープを1回だけ読み、後の`set_root`は何も変えない。worker 0のループの根は、次の状態を順にたどる。

- `Main`: ループは最初に`main`を動かす
- `None`: `main`は根のスコープを置かなかった。worker 0のループは`main`の後で戻る
- `Open`: `main`が根のスコープを置いた。スコープは、最後のタスクが終わったときに閉じる
- `Closed`: 判定の登録表の区間が、`Registry::closed`でスコープの閉じたことを読んだ。その後で結果の項`Item::Root(Root::Result)`が動く
- `Done`: 結果の項が動き、worker 0のループは戻る

閉じた根は、判定をほかの何よりも先に終わらせる。ほかのworkerが`running`に数えられておらず、一覧に何もなく、worker自身の事実が静かなとき、判定は期限、操作、シグナルを待つタスク、`holds`を見ずに`RootClosed`に決める。閉じた根は、静かでない自分の事実 (判定はやり直す) にも、止まりにも勝たない。

## 判定

準備のできた項がないとき、worker 0は判定を動かす。判定は`eml_steps::verdict::step`のインタプリタである。worker 0の眠りは判定で、それが動く間、phaseは`Idle`である。インタプリタは効果を行う前に、それを`Point::Kernel(Label::Verdict(effect))`として方針に知らせる。各効果は1回のアトミックな読みか1つの区間で、インタプリタはそこで読んだものだけを返す。runがworkerを1つ持つ間は、`Own`が例外である。まだslotのlockがないので、mail、止める語、workerが記録した止まりを順に読む。止めるビットを立てるシグナルハンドラのほかに、それらを書くスレッドはない。runの時計 (仮想か本物か) はrunの間変わらず、最初の`pc`とともに入る。今のrunはworkerを1つ持つので、効果は次の事実を読む。

- `Count`: ほかのworkerは`running`に数えられない。runにほかのworkerはないからである
- `Lists`: 一覧に仕事はない。開いた一覧はないからである
- `Registry`: 1つの登録表の区間で、根のスコープが閉じたか、期限で眠るタスクがあるか、進行中の操作があるか、中継するシグナルを待つタスクがあるかを読む
- `Own`: mailに項があるか、`NOTICE`、止まり (止めるビットか、このworkerが記録した止まり)

`holds`はまだ事実ではない。runにメインのworkerがないからである。判定の待ち (`Wait`、`WaitUntil`、`Advance`) はまだ書いていない。事実が待ちを求めたとき (期限で眠るタスク、driverの中の操作、中継するシグナルを待つタスク)、runは`Deadlock`ではなく`Internal("the verdict asked for a wait this scheduler does not have yet")`で終わる。そうでなければ判定は、`RootClosed` (根は`Closed`になる)、`Deadlock` (`RunFault::Deadlock`として記録する)、`Restart`か`Stop` (ループに戻る) で終わる。

## runの手順

`sequence::run`は、runのただ1つの手順である。`drive`を呼ぶのはこれだけで、ただ1つの`Control::clear_end`を持つ。エンジンは、自分のworkerのfactory (`Workers`) と`Start`を渡してこれを呼ぶ。`Start`は、runのすべての`Ctx`の所有者の表、host、`RunConfig`で、`RunConfig`の`control`がrunの`Control`である。今は合わせた席で1つのworkerを動かし、呼ぶスレッドがworker 0である。`Control`のworkerの数がほかの数のrunと、席が分かれたrunは、何も始めず、`Internal("this run sequence runs one worker in the merged seat")`を返す。

`Workers::session`が真のfactoryは、セッションの`Ctx`のように、runより長く生きる`Ctx`をworker 0に貸す。そのrunは、合わせた席でworkerを1つだけ持つ。ほかの数のworkerか分かれた席では、`sequence::run`は何も始めず、`Internal("a session run has one worker")`を返す。ほかのworkerを持つrunの後では、生き残るヒープの物体がほかのworkerのページにありうるので、後の`Ctx::recover`がそのページを読むことになるからである。そのとき`Start.owners`は、貸した`Ctx`のヒープの表である。止まったrunの後、貸す側は、もう一度貸す前に`Ctx`を回復するか落とす。worker 0で`Halt::Panicked`があった後の貸した`Ctx`は、貸す側が落とさず、たどらず、回復しないものである。

1. `Control::begin_run`がrunの終わりの欄を開く
2. runは呼ぶスレッドで、`Workers::policy`が返す方針でworker 0の`WorkerCore`を作り、runのcallbackと`Env`を渡して`Workers::on_worker`を呼ぶ。`Env`は核とrunの共有の部分を渡す。factoryは、runの所有者の表の上に`Ctx::for_worker`で`Ctx`を作り、自分の`Exec`を組み、それへのポインタでcallbackを1回呼び、核をそのスレッドで落とし、`Ctx`を切り離して持つ`Retired`を返す
3. callbackがループを動かす。ループが戻ったときが、理由によらず、runを終わらせる出来事である。panicを記録し、`Control::end_run`がすべての止める語にENDを立てる。記録は自分のcatchの下で動き、ENDはその後で立てるので、記録がpanicしてもENDは立つ。callbackを呼ばずに戻ったfactoryも同じ出来事で、`Internal("a worker left without running")`として記録する
4. `finish`が、呼ぶスレッドでrunを確定させる。最初に記録した止まりが勝ち、終了コードは`Control::exit_code`である。その後で`Control::clear_end`がENDを下ろす。確定がpanicしてもENDは下りる。確定は自分のcatchの下で動き、`clear_end`はcatchが何を返してもその後で動くからである。`finish`は、呼ぶスレッドのcallbackの終わりで動くか、そのcallbackが動かなかったときは、factoryが戻った後に`sequence::run`から動く
5. `sequence::run`は`Outcome { stop, exit, retired }`を返す。`retired`は、すべてのworkerの`Retired`をその場所とともに持つ

runの登録表 (`cells::Shared`) とそのmailは、runの中にある。runが終わると、登録表とすべての`WorkerCore`を落とし、それらが持つ語には触れない。

記録するのは`Stopped { worker, cause }`である。`cause`は、エンジンがそのworkerの`Retired`に誤りを持つときは`Cause::Halt`、スケジューラ自身の理由のときは`Cause::Fault(RunFault)`である。最初に書いたものが勝つ。

### panic

ほかのworkerがまだ動きうる間は、どの巻き戻しも`Ctx`もヒープも落とさない。ほかのスレッドが共有の物体を持っているヒープは落としてはならないからである ([オブジェクトモデル](object-model.md)の「`share`」)。そのため次のようにする。

- `Halt`は、受け取る側が何をしてよいかで定める。`Stopped`は、workerが止まり、そのヒープが操作の間にあることを表す。`Panicked`は、workerが止まり、そのヒープが操作の途中にあるかもしれないことを表す。受け取る側はそのヒープを切り離し、その中の何もたどらず、確かめない。`Exec::enter`はどちらを返してもよく、エントリがRustのpanicを捕まえたエンジンは`Panicked`を返す。ループはそれを変えずに渡す。3つ目の変種はない
- runのcallbackは、すべての文がcatchの下にある1つの関数である。ループ、ループの終わり、`finish`はそれぞれ自分のcatchの下で動くので、そのどれかのpanicは`Internal("a worker thread panicked")`として記録され、後のものもそのまま動く。そのときcallbackは`Err(Halt::Panicked)`をfactoryに返す
- `Err(Halt::Panicked)`のとき、factoryは`Ctx`を切り離して包むだけで、それでも`Retired`を返す。`Outcome.retired`はすべてのworkerを持ち、panicしたworkerのヒープをどうするかは、runの後に呼ぶ側が決める
- `Ctx`を切り離すとは、それをどのスレッドのエンジンの状態からも外し、落とさずに`Retired`に入れることである。切り離した`Ctx`を落とすか、たどるか、回復するかは、runの後に呼ぶ側が決める。エンジンのエントリがヒープの操作の途中でpanicしうるときは、`Halt`が`Panicked`だったworkerについて、呼ぶ側はそのどれもしない。`Scripts`の項は閉包なので、`Halt::Panicked`の後のヒープが操作の途中にあるのは、閉包が操作の途中でpanicしたときだけである。そうしない閉包のテストは、戻ってきた`Ctx`を落としてよい
- runが持つ`Retired`はどれも、それを受け取る式から`Outcome`に移すまで`ManuallyDrop`の中にあり、その間の手順はどれもcatchの下で動く。だからそのようなフレームの巻き戻しは、ヒープを落とさずにリークする
- factoryはcallbackの外でpanicせず、フレームの巻き戻しが`Ctx`を落とさずにリークするように`Ctx`を持つ。それでもフレームが巻き戻ったら、`sequence::run`はスレッドの根元で巻き戻しを捕まえ、`eml: a worker's set-up panicked outside its loop; aborting`をstderrに書き、プロセスを中止する。巻き戻しは`Ctx`をリークしたので、中止の前にほかのスレッドが解放した記憶を読むことはなく、中止は定義された終わりである。同じくcallbackの外で動く`Workers::policy`と`Workers::session`のpanicも、同じように中止する
- 捕まえたpanicのpayloadは、自分のcatchの下で落とす。その落とし方の中のpanicのpayloadは忘れる。どのpanicも`sequence::run`の外に出ない

## lockの順位

スケジューラのlockはどれも`rank.rs`を通して、`PoisonError::into_inner`で取る。lockの下でpanicしたスレッドは、状態を1回の代入が残すとおりに残したからである。順位は2つある。

| 順位 | lock |
|---|---|
| 1 | 登録表 (`cells::Shared`) |
| 2 | スケジューラのほかのすべてのlock。今は止まりの記録、runの確定した終わり、`Scripts`が貸す`Ctx`である |

順位2のlockは、ほかの順位2のlockを持たないときにだけ取り、順位2のlockを持つ間は登録表を取らない。だからスレッドが持つのは、多くても登録表と、その下の1つの順位2のlockである。登録表を中でlockする`cells`の関数を呼ぶことも、登録表を取ることに数える。デバッグのビルドでは、スレッドローカルな値がスレッドの持つlockを記録し、表明が違反を止める。同じ表明が、`Policy::at`をlockを持たずに呼ぶことを確かめる。リリースのビルドは何も記録しない。

スケジューラのファイルは、登録表を`rank::registry`で取り、`&Shared`を取る`cells`の関数をどれも`rank::cells`を通して呼ぶ。どちらも、順位2のlockを持たず、登録表もまだ持たないことを表明する。`cells`は登録表を持ったまま`Mailer`のメソッドを呼ぶので、そのどれもが`rank::under_registry`で順位1の印を付ける。この印は真偽ではなく、guardが上げ下げする深さである。スケジューラのファイルの呼び出しは入れ子になり (`rank::cells`、`cells`、`Mailer`の順)、エンジンの行の呼び出しは`Mailer`で1回だけ印を付けるからである。`WorkerCore::shared`を通して登録表をlockするエンジンの行は、表明の外にある。それでよいのは、スケジューラのほかのlockが`eml_sched`の外に見えず、行はそのどれも持てないからである。`Mail`と`HostBin`のlockは`cells`の中にあって何も呼び返さないので、順位の外にある。

## フック

`Policy::at(&self, worker, point)`は、その地点を行うworkerが、行う前に、lockを持たずに呼ぶ。`Point::Kernel(label)`は`eml_steps`のカーネルの手順か効果 (`eml_steps::Label`) で、それを行うインタプリタの頭にある1つの文が知らせる。`Point::Shell(point)`は、どのカーネルも決めない、シェルの4つの地点の1つである。`JoinWait` (`wait_join`が待つ前)、`Submit` (hostが操作を自分のdriverに出す前)、`HandBack` (hostが完了を持ち主に渡すために`cells`を呼ぶ前)、`Leave` (workerの最後の`wake_all`の前) である。今はループが自分の手順を、判定が自分の効果を知らせ、シェルの地点はまだ知らせない。

方針は、スケジューラが読むものを何も決めない。テストは、`at`の中でworkerを止めて、交互の順を強いる。`Sched::Normal`と`Sched::Sequential`の方針である`InOrder`は、何も見ない。

`Signs`は各workerのフックの値を自分の`Arc`に持つので、runの後もそれを持つ読み手が届くのは、workerごとに2つのアトミックな値だけで、ヒープの語には届かない。`WorkerCore::signs`がそれを返す。`current`は、エントリが動いているタスクのセルの番号の即値か`NO_CELL`で、項を開くときと閉じるときに書く。phase (`Phase`) は、項を開いてから`Run`、worker 0が判定を動かす間は`Idle`、その後でまた`Run`である。`Phase::Io`は宣言してあり、何も書かない。スケジューラのどれも、どちらの値も読まない。

## 信頼の一覧

Verusが`eml_rt`と`eml_steps`について証明することのほかに、スケジューラの記憶の安全が拠りどころにするもの。スケジューラ自身のファイルの`unsafe`のブロックはどれも、拠りどころにする行か約束を名指す。`doorbell.rs`のブロックはSM-8に、`cells`のブロックはそのmoduleの「呼ぶ側の約束」に拠る。

A群、シェルによるカーネルの実行。

- ST-1: カーネルが返す効果と手順はどれも、1回のアトミックな操作か1つのlockの区間として行い、返す事実はその区間の中で読む
- ST-2: カーネルのモデルは逐次一貫性を前提にする。`sleepers`、`searching`、`running`、`owed`は`SeqCst`である。走査は一覧を、その欄のlockの下でだけ読む。`listed`をlockなしで読むのは判定だけで、`running`の後である。ST-2が名指す欄は、それを入れるコードとともに来て、この行はそのコードを縛る
- ST-3: `std::sync::Mutex`、`Condvar`、スレッドのparkは、文書のとおりに動く

B群、スケジューラの境界の約束。

- SM-5、`Exec`の約束: `with`は、このworkerのスレッドの核と`Ctx`を自分の閉包にだけ貸し、閉包が戻った後はどちらの借用も持たない。`enter`をまたいで、呼ぶ側は`Exec`のポインタから導いたものを何も持たない。`enter`はその`Ctx`の上で項を1つ動かし、panicを捕まえたときは`Halt::Panicked`を返す
- SM-6、`Marked`の作り方: `cells`の外では、語は`Marked`としてだけworkerの間を移り、`Marked`の作り方はどれも、語を移せる形にした`share`を名指す。`Marked`の作り方はまだない
- SM-7、`cells`の約束の広げ方と`here`: `sequence::run`を呼ぶ側は、`cells`のC3が登録表について請け負うように、エンジンがrunの`WorkerCore`を通して行うことをすべて請け負う。`schedule`で置く語と、`Here`とともに`cells`の関数に渡す語はC2を、渡す`Ctx`はC1を守り、登録表はC1からC4を守る呼ぶ側だけが動かす。`WorkerCore::here`は`unsafe fn`で、その結果は`cells`の関数にだけ渡す。だから`Here`の`post` (`Mailer`) を呼ぶのは`cells`の関数だけで、登録表を持ったまま呼ぶ
- SM-8、`doorbell.rs`の`libc`の呼び出し: doorbellが持つ2つのfdへの`pipe`、`fcntl`、`write`、`poll`、`read`、`close`と、OSの場所を返す関数を通した`errno`である
- SM-9、`Workers`の約束: `on_worker`は、workerの自分のスレッドで1回だけ動く。callbackを呼ぶ前に、そのスレッドで、runの所有者の表の上に`Ctx::for_worker`で`Ctx`を作る。callbackは多くても1回呼ぶ。callbackの外でpanicせず (`Workers::policy`と`Workers::session`も)、巻き戻しが`Ctx`をリークするように持つ。核をそのスレッドで落とし、`Ctx`を切り離して落とさずに`Retired`に入れて返す。ただし`Workers::session`が真なら、worker 0は呼ぶスレッドの、runより長く生きる`Ctx`を使ってよく、その`Ctx`は`Retired`に入れない。止まったrunの後、貸す側は、もう一度貸す前に`Ctx`を回復するか落とす。worker 0で`Halt::Panicked`があった後の貸した`Ctx`は、貸す側が落とさず、たどらず、回復しないものである
