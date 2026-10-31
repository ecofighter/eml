//! 実行の制御。セッションに1つあり、CLI と、どの worker のヒープも `Arc` で同じものを見る。実行を止める理由の語と、
//! セッションのヒープが OS から取ったページのバイト数と、上限と、検査モードの論理的な worker ID の表を持つ
//! (docs/spec/object-model.md の「実行の制御」「論理的な worker ID」)。

use std::collections::BTreeMap;
use std::fmt;
use std::ptr;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use vstd::prelude::*;

#[cfg(verus_only)]
use crate::alloc::page_base;
use crate::error::HeapError;
use crate::heap::Heap;
use crate::mode::Mode;

verus! {

/// 実行の上限。`Control` を作るときに決め、変えない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// セッションのヒープが OS から取るページのバイト数の上限。
    pub max_bytes: usize,
    /// 深さの上限。VM のタスクのスタックの語の数で数える。
    pub max_depth: usize,
    /// `evv` の長さの上限。
    pub max_evv: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_bytes: usize::MAX, max_depth: usize::MAX, max_evv: usize::MAX }
    }
}

/// 実行を止めた理由。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Interrupted,
    Terminated,
    DepthLimit,
    MemoryLimit,
    Ended,
    Preempt,
    /// worker が次の安全点で知らせを集めること。止めずに続けるので、利用者には見せない。
    Notice,
}

pub struct Control {
    stop: AtomicU32,
    worker: AtomicU32,
    end: AtomicU32,
    code: AtomicI64,
    bytes: AtomicUsize,
    signals: AtomicU32,
    watched: AtomicU32,
    limits: Limits,
    workers: WorkerIds,
    pages: PageMap,
    /// ページの表の登録の証が、どの `Control` のものかを見分ける番号。
    // 仕様だけが読む
    #[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
    id: u64,
}

/// 検査モードの論理的な worker ID の表 (docs/spec/object-model.md の「論理的な worker ID」)。`Control` に置き、同じ
/// `Control` のどのヒープも同じ表で数える。回復だけが消し、run ごとには消さない。
// 中身は Verus に見せない (信頼の一覧の T15)。数は証明の対象にせず、`Control::check_owned` が数え直して確かめる
#[verifier::external_body]
pub(crate) struct WorkerIds {
    /// まだ出していない最小の ID。0 は基の ID なので 1 から始める。
    next: AtomicU32,
    /// 退いて使い回せる ID。最後に退いたものが末尾にある。
    free: Mutex<Vec<u32>>,
    /// ID ごとの、rc が正の生きている物体の数。`CHUNK_IDS` 個ずつの塊に分け、塊を `Arc` で各ヒープの写しと分け合う。
    chunks: Mutex<Vec<Arc<Chunk>>>,
}

/// 検査モードのページの表 (docs/spec/object-model.md の「ページの表」)。同じ `Control` のヒープが取ったページを、
/// 凍結した塊のページも含めて登録する。検査モードの操作は、即値でない語をたどる前にそのページが登録されているかを
/// 確かめる。速いモードは触れない。
// 64 KiB のページの番号 (番地 >> 16) を、6 ビットずつの上の3段と下位 14 ビットの葉で、4段に引く基数の表である。
// 中身は Verus に見せない (信頼の一覧の T15)
#[verifier::external_body]
pub(crate) struct PageMap {
    top: OnceLock<Box<Node<Node<Node<Leaf>>>>>,
}

/// `control` の表に番地 `page` のページを登録した証。登録したヒープの確保器がページごとに1つ持ち、ページを返す前に
/// `unregister_page` に渡して手放す。作れるのは `register_page` だけである (信頼の一覧の T15)。
#[cfg_attr(not(verus_keep_ghost), allow(dead_code))]
pub(crate) tracked struct Registered {
    ghost page: usize,
    ghost control: u64,
}

impl Registered {
    /// 登録したページの番地。
    pub(crate) closed spec fn page(&self) -> usize {
        self.page
    }

    /// 登録した `Control` の番号 (`Control::ident`)。
    pub(crate) closed spec fn control(&self) -> u64 {
        self.control
    }
}

/// 番地 `a` のブロックのページが、`control` のページの表に登録されているか (T15)。表だけを読み、`a` の指すメモリには
/// 触れない。約束は1つで、`a` のページを `control` に登録した証 `t` を渡せば真を返す。証があるうちは、そのページを
/// 外すのは証を手放す `unregister_page` だけで、ページは確保したままなので同じ番地をほかの登録が使うこともない。
#[verifier::external_body]
#[inline]
pub(crate) fn page_known(
    control: &Control,
    a: usize,
    Tracked(t): Tracked<Option<&Registered>>,
) -> (r: bool)
    ensures
        t matches Some(t) && t.page() == page_base(a) && t.control() == control.ident() ==> r,
{
    control.pages.contains(a)
}

/// 番地 `page` のページを `control` のページの表に登録し、その証を返す (T15)。ページを確保した直後に、ページごとに
/// 1回だけ呼ぶ。
#[verifier::external_body]
pub(crate) fn register_page(control: &Control, page: usize) -> (t: Tracked<Registered>)
    ensures
        t@.page() == page,
        t@.control() == control.ident(),
{
    control.pages.insert(page);
    Tracked::assume_new()
}

/// 番地 `page` のページを `control` のページの表から外し、証を手放す (T15)。ページを返す前に呼ぶ。
#[verifier::external_body]
pub(crate) fn unregister_page(control: &Control, page: usize, Tracked(t): Tracked<Registered>)
    requires
        t.page() == page,
        t.control() == control.ident(),
{
    control.pages.remove(page);
}

/// 新しい `Control` の番号。プロセスの中で同じ番号を2度出さない (T15)。
#[verifier::external_body]
fn fresh_control_id() -> u64 {
    CONTROL_IDS.fetch_add(1, Ordering::Relaxed)
}

/// ヒープが持つ、`WorkerIds` の数の塊の写し (docs/spec/object-model.md の「論理的な worker ID」)。数を変えるたびに
/// 表の lock を取らないように、塊の `Arc` を写して持ち、写しにない ID が来たときだけ写し直す。速いモードのヒープは
/// 何も持たない写しのまま触れない。
#[verifier::external_body]
pub(crate) struct IdCounts {
    control: Option<Arc<Control>>,
    chunks: Vec<Arc<Chunk>>,
}

impl PageMap {
    #[verifier::external_body]
    pub(crate) fn empty_map() -> PageMap {
        PageMap { top: OnceLock::new() }
    }
}

impl WorkerIds {
    #[verifier::external_body]
    pub(crate) fn empty_table() -> WorkerIds {
        WorkerIds {
            next: AtomicU32::new(1),
            free: Mutex::new(Vec::new()),
            chunks: Mutex::new(Vec::new()),
        }
    }
}

// どの関数も値について何も約束しない (T15)。`fork_id` と `retire_id` は前提が破れていれば状態を変えずに `None` を返し、
// 呼ぶ側が T5 の関数で止める
impl IdCounts {
    /// 何も持たない写し。速いモードのヒープと、`share` と `transfer_back` が写しをヒープから取り出している間の
    /// 置き場が使う。
    #[verifier::external_body]
    pub(crate) fn detached() -> IdCounts {
        IdCounts { control: None, chunks: Vec::new() }
    }

    /// `control` の表の写し。基の ID 0 を使っている ID にする。
    #[verifier::external_body]
    pub(crate) fn for_control(control: &Arc<Control>) -> IdCounts {
        let mut counts = IdCounts { control: Some(Arc::clone(control)), chunks: Vec::new() };
        counts.enter_id(0);
        counts
    }

    /// `id` を使っている ID にする。まだ出していない ID からも退いた ID からも外し、数の塊を `id` まで伸ばす。
    #[verifier::external_body]
    pub(crate) fn enter_id(&mut self, id: u32) {
        if let Some(control) = &self.control {
            let ids = &control.workers;
            let mut free = lock(&ids.free);
            ids.next.fetch_max(id.saturating_add(1), Ordering::Relaxed);
            if let Some(i) = free.iter().position(|f| *f == id) {
                free.remove(i);
            }
            drop(free);
            self.chunks = ids.grow(id);
        }
    }

    /// `id` の数を1つ増やすか減らす。どのヒープも使っていない ID は数えない。壊れた前置きの所有者を読んでも大きな
    /// 表を確保せず、数え直しがその ID の `OwnedMiscount` を返すためである。
    // 数は証明の対象にしないので、桁あふれで panic する道を作らないように wrapping で数える (`fetch_add` は回る)
    #[verifier::external_body]
    pub(crate) fn count_id(&mut self, id: u32, up: bool) {
        let (c, i) = (id as usize / CHUNK_IDS, id as usize % CHUNK_IDS);
        if c >= self.chunks.len() {
            match &self.control {
                Some(control) => self.chunks = lock(&control.workers.chunks).clone(),
                None => return,
            }
            if c >= self.chunks.len() {
                return;
            }
        }
        let n = &self.chunks[c][i];
        if up {
            n.fetch_add(1, Ordering::Relaxed);
        } else {
            n.fetch_sub(1, Ordering::Relaxed);
        }
    }

    /// 最後に退いた ID か、なければまだ出していない最小の ID を出す。ID が `u32` の上限に届いていれば `None` を返す。
    #[verifier::external_body]
    pub(crate) fn fork_id(&mut self) -> Option<u32> {
        let control = self.control.as_ref()?;
        let ids = &control.workers;
        let mut free = lock(&ids.free);
        let id = match free.pop() {
            Some(id) => id,
            None => {
                let id = ids.next.load(Ordering::Relaxed);
                if id == u32::MAX {
                    return None;
                }
                ids.next.store(id + 1, Ordering::Relaxed);
                id
            },
        };
        drop(free);
        self.chunks = ids.grow(id);
        Some(id)
    }

    /// `id` を退かせる。`id` が 0 か、出していないか、もう退いていれば `None` を返す。そうでなければ `id` の数を返し、
    /// 数が 0 のときだけ `id` を使い回せるようにする。
    ///
    /// 呼ぶ前に、ID を出してから `id` の数を変えたどのスレッドの書き込みも、この呼び出しより前に起きていること
    /// (happens-before) を、呼ぶ側が保つ。数は Relaxed で数えるので、この表は順序を作らない。`par` では、子が結果を
    /// Release で公開し、親が Acquire で読んでから `transfer_back` とこの呼び出しをする
    /// (docs/spec/object-model.md の「論理的な worker ID」)。
    #[verifier::external_body]
    pub(crate) fn retire_id(&self, id: u32) -> Option<u64> {
        let control = self.control.as_ref()?;
        let ids = &control.workers;
        let mut free = lock(&ids.free);
        if id == 0 || id >= ids.next.load(Ordering::Relaxed) || free.contains(&id) {
            return None;
        }
        let live = ids.live(id);
        if live == 0 {
            free.push(id);
        }
        Some(live)
    }

    /// 表を消す。数をすべて 0 にし、出した ID と退いた ID を忘れる。回復が写す前に呼ぶ
    /// (docs/spec/object-model.md の「回復」)。
    #[verifier::external_body]
    pub(crate) fn reset_ids(&self) {
        if let Some(control) = &self.control {
            let ids = &control.workers;
            let mut free = lock(&ids.free);
            free.clear();
            ids.next.store(1, Ordering::Relaxed);
            for chunk in lock(&ids.chunks).iter() {
                for n in chunk.iter() {
                    n.store(0, Ordering::Relaxed);
                }
            }
        }
    }
}

impl Control {
    pub fn new(limits: Limits) -> Control {
        Control {
            stop: AtomicU32::new(0),
            worker: AtomicU32::new(0),
            end: AtomicU32::new(0),
            code: AtomicI64::new(0),
            bytes: AtomicUsize::new(0),
            signals: AtomicU32::new(0),
            watched: AtomicU32::new(0),
            limits,
            workers: WorkerIds::empty_table(),
            pages: PageMap::empty_map(),
            id: fresh_control_id(),
        }
    }

    /// この `Control` の番号。
    pub(crate) closed spec fn ident(&self) -> u64 {
        self.id
    }

    /// 上限のない新しい `Control`。
    // `Heap::new` はモードごとに呼ぶ側の crate で写される。`Arc` を作る処理をその写しに含めると、計測のプログラムの
    // コード生成単位の分け方が変わり、速い道が呼ぶ側に展開されなくなったので、モードによらないこの関数に置く
    // (docs/implementation/benchmarks.md の「`eml_rt` のインライン化」)
    pub(crate) fn unbounded() -> Arc<Control> {
        Arc::new(Control::new(Limits::default()))
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }
}

} // verus!
// VM も、安全点で止めたときにこの文言を出す
impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Stop::Interrupted => "interrupted",
            Stop::Terminated => "terminated",
            Stop::DepthLimit => "the call depth limit is exceeded",
            Stop::MemoryLimit => "the memory limit is exceeded",
            Stop::Ended => "the run has ended",
            Stop::Preempt => "preempted",
            Stop::Notice => "a signal notice",
        })
    }
}

// 止める語が伝えるのは「止めよ」のビットだけで、確保量の数が伝えるのもその値だけである。シグナルの語と見張りの語も
// ビットだけを伝える。どれにも、ほかのメモリへの書き込みを公開する役はない。そのため、どの読み書きも Relaxed で足りる
impl Control {
    pub const INTERRUPT: u32 = 1;
    pub const MEMORY: u32 = 2;
    pub const TERMINATE: u32 = 8;
    /// worker の止める語にだけ立つ。次の安全点で知らせを集めよ、という意味である。
    pub const NOTICE: u32 = 1 << 30;
    /// worker の止める語にだけ立つ。`stop` には立てない (host の読み手が見る語を変えないため)。
    pub const PREEMPT: u32 = 1 << 31;
    /// シグナルの語で、中継済みのビットを保留のビットから離す幅。
    const RELAYED_SHIFT: u32 = 16;
    const PENDING_MASK: u32 = (1 << Self::RELAYED_SHIFT) - 1;

    /// 止めるビットを `stop` と worker の語の両方に直接書く。worker の語へ写す仲介を置かず、止まる時点を書いた時点で決める。
    fn raise(&self, bits: u32) {
        self.stop.fetch_or(bits, Ordering::Relaxed);
        self.worker.fetch_or(bits, Ordering::Relaxed);
    }

    pub fn interrupt(&self) {
        self.raise(Self::INTERRUPT);
    }

    pub fn terminate(&self) {
        self.raise(Self::TERMINATE);
    }

    /// worker に、次の安全点で切り替えるよう求める。
    pub fn preempt(&self) {
        self.worker.fetch_or(Self::PREEMPT, Ordering::Relaxed);
    }

    /// 切り替えを受けたときに `PREEMPT` を下ろす。
    pub fn take_preempt(&self) {
        self.worker.fetch_and(!Self::PREEMPT, Ordering::Relaxed);
    }

    /// worker に、次の安全点で知らせを集めるよう求める。
    pub fn notice(&self) {
        self.worker.fetch_or(Self::NOTICE, Ordering::Relaxed);
    }

    /// 知らせを集めるときに `NOTICE` を下ろす。
    pub fn take_notice(&self) {
        self.worker.fetch_and(!Self::NOTICE, Ordering::Relaxed);
    }

    /// シグナル `bits` (`INTERRUPT` か `TERMINATE`) を、待つタスクへ届けるまで保留にする。
    pub fn defer(&self, bits: u32) {
        self.signals.fetch_or(bits, Ordering::Relaxed);
    }

    /// まだ届けていない (保留の) シグナルのビット。中継済みのものも含む。
    pub fn pending_signals(&self) -> u32 {
        self.signals.load(Ordering::Relaxed) & Self::PENDING_MASK
    }

    /// 保留のうちまだ中継していないビットを中継済みにして返す。1回の保留につき、返すのは1回だけである。
    pub fn claim_signals(&self) -> u32 {
        let mut new = 0;
        let _ = self
            .signals
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |s| {
                new = s & Self::PENDING_MASK & !(s >> Self::RELAYED_SHIFT);
                (new != 0).then_some(s | new << Self::RELAYED_SHIFT)
            });
        new
    }

    /// 届けたシグナル `bits` の保留と中継済みを下ろす。
    pub fn settle(&self, bits: u32) {
        self.signals
            .fetch_and(!(bits | bits << Self::RELAYED_SHIFT), Ordering::Relaxed);
    }

    /// シグナルの語を空にし、保留だったビットを返す。
    pub fn take_pending(&self) -> u32 {
        self.signals.swap(0, Ordering::Relaxed) & Self::PENDING_MASK
    }

    /// 待つタスクのいるシグナルのビットを書く。handler はこれを読んで、止めるか保留にするかを決める。
    pub fn set_watched(&self, bits: u32) {
        self.watched.store(bits, Ordering::Relaxed);
    }

    pub fn watched(&self) -> u32 {
        self.watched.load(Ordering::Relaxed)
    }

    // `Ctx::poll` が読む。`stop_bits` と同じく、`#[inline]` がないとほかの crate から展開できない
    #[inline]
    pub fn worker_bits(&self) -> u32 {
        self.worker.load(Ordering::Relaxed)
    }

    /// `exit n` の終わりを書く。最初の書き手だけが `code` を書いて真を返し、後の書き手は何も変えずに偽を返す。
    pub fn exit(&self, code: i64) -> bool {
        if self
            .end
            .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        self.code.store(code, Ordering::Relaxed);
        true
    }

    /// 終わりが書かれていれば、その終了コード。worker が戻った後で host が読む。
    pub fn exit_code(&self) -> Option<i64> {
        match self.end.load(Ordering::Relaxed) {
            0 => None,
            _ => Some(self.code.load(Ordering::Relaxed)),
        }
    }

    /// run の始めに終わりの欄を開き、`PREEMPT` と `NOTICE` を下ろす。止めるビットとシグナルの語は下ろさない。
    pub fn begin_run(&self) {
        self.end.store(0, Ordering::Relaxed);
        self.worker
            .fetch_and(!(Self::PREEMPT | Self::NOTICE), Ordering::Relaxed);
    }

    /// 止める理由のビットとシグナルの語と見張りの語をすべて下ろし、終わりの欄を開く。確保量が上限を下回ってもビットは
    /// 下りないので、止めるビットを下ろすのはこの関数だけである。`PREEMPT` と `NOTICE` は `take_preempt`、`take_notice`、
    /// `begin_run` も下ろす。
    pub fn clear(&self) {
        self.stop.store(0, Ordering::Relaxed);
        self.worker.store(0, Ordering::Relaxed);
        self.end.store(0, Ordering::Relaxed);
        self.signals.store(0, Ordering::Relaxed);
        self.watched.store(0, Ordering::Relaxed);
    }

    // `Ctx::poll` は VM の呼び出しと戻りごとにこれを読む。総称でない関数は、`#[inline]` がないとほかの crate から
    // 展開できない
    #[inline]
    pub fn stop_bits(&self) -> u32 {
        self.stop.load(Ordering::Relaxed)
    }

    /// セッションのヒープが OS から取ったページのバイト数。
    pub fn bytes(&self) -> usize {
        self.bytes.load(Ordering::Relaxed)
    }

    /// ページを取った分を足し、上限を超えたら `MEMORY` を立てる。
    pub(crate) fn charge(&self, bytes: usize) {
        let total = self
            .bytes
            .fetch_add(bytes, Ordering::Relaxed)
            .saturating_add(bytes);
        if total > self.limits.max_bytes {
            self.raise(Self::MEMORY);
        }
    }

    /// 今の確保量に `bytes` を足すと上限を超えるか。確保より前に大きさが分かるとき、確保を試す前に止めるのに使う。
    pub(crate) fn over_budget(&self, bytes: usize) -> bool {
        self.bytes().saturating_add(bytes) > self.limits.max_bytes
    }

    pub(crate) fn refund(&self, bytes: usize) {
        self.bytes.fetch_sub(bytes, Ordering::Relaxed);
    }
}

/// 数の塊1つが受け持つ ID の数。
// Miri では、塊の大きさに比例して数える費用が増える。1024 個にすると、検査モードの `trusted` のテスト1本が
// Tree Borrows で2倍の時間になったので、小さく保つ
const CHUNK_IDS: usize = 64;

type Chunk = [AtomicU64; CHUNK_IDS];

/// 次に作る `Control` の番号。
static CONTROL_IDS: AtomicU64 = AtomicU64::new(0);

/// ページの表の上の3段の、1つの表の項目の数。
const MAP_FAN: usize = 64;
/// ページの表の葉の語の数。1つの葉が 2^14 個のページ (1 GiB) を受け持つ。
// Miri では、表を作る費用が表の大きさによらないように、どの表も繰り返しの式で作る。表を落とすときは項目を1つずつ
// 落とすので、項目の数は小さく保つ
const LEAF_WORDS: usize = 256;

type Leaf = [AtomicU64; LEAF_WORDS];
type Node<T> = [OnceLock<Box<T>>; MAP_FAN];

/// ページの番号を、3つの段の添字と、葉の語と、語の中のビットに分ける。番号が表の外 (番地が 2^48 以上) なら `None`。
fn map_slot(a: usize) -> Option<([usize; 3], usize, u64)> {
    let n = a >> 16;
    (n >> 32 == 0).then(|| {
        let bit = n % (LEAF_WORDS * 64);
        let step = |k: u32| (n >> (14 + 6 * k)) % MAP_FAN;
        ([step(2), step(1), step(0)], bit / 64, 1u64 << (bit % 64))
    })
}

/// 項目がみな空の表。
fn empty_node<T>() -> Box<Node<T>> {
    Box::new([const { OnceLock::new() }; MAP_FAN])
}

// 登録のビットは、ページの物体の語をほかのスレッドへ渡す前に立て、ページを返す前に下ろす。語を受けたスレッドは、
// 語を渡したときの順序でビットを見るので、表の読み書きは Relaxed で足りる
/// 番地 `a` のページが表にあるか。値の約束を持たない読みで、T14 の `foreign_header` が使う。
pub(crate) fn control_knows(control: &Control, a: usize) -> bool {
    control.pages.contains(a)
}

/// `Heap` を落とすときに、確保器の `Drop` がページを返す前に表から外す。登録の証は確保器とともに消える。
pub(crate) fn forget_page(control: &Control, page: usize) {
    control.pages.remove(page);
}

impl PageMap {
    fn leaf(&self, a: usize) -> Option<(&AtomicU64, u64)> {
        let ([i, j, k], word, bit) = map_slot(a)?;
        let leaf = self.top.get()?[i].get()?[j].get()?[k].get()?;
        Some((&leaf[word], bit))
    }

    fn contains(&self, a: usize) -> bool {
        self.leaf(a)
            .is_some_and(|(word, bit)| word.load(Ordering::Relaxed) & bit != 0)
    }

    fn insert(&self, a: usize) {
        let ([i, j, k], word, bit) =
            map_slot(a).expect("a page below 2^48 for the checked-mode page map");
        let leaf = self.top.get_or_init(empty_node)[i].get_or_init(empty_node)[j]
            .get_or_init(empty_node)[k]
            .get_or_init(|| Box::new([const { AtomicU64::new(0) }; LEAF_WORDS]));
        leaf[word].fetch_or(bit, Ordering::Relaxed);
    }

    fn remove(&self, a: usize) {
        if let Some((word, bit)) = self.leaf(a) {
            word.fetch_and(!bit, Ordering::Relaxed);
        }
    }
}

// 表の lock の中では panic しないので、毒の入った lock も中身はそのまま使える
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl WorkerIds {
    /// 数の塊を `id` まで伸ばし、塊の一覧の写しを返す。
    fn grow(&self, id: u32) -> Vec<Arc<Chunk>> {
        let mut chunks = lock(&self.chunks);
        while chunks.len() <= id as usize / CHUNK_IDS {
            chunks.push(Arc::new(std::array::from_fn(|_| AtomicU64::new(0))));
        }
        chunks.clone()
    }

    fn live(&self, id: u32) -> u64 {
        lock(&self.chunks)
            .get(id as usize / CHUNK_IDS)
            .map_or(0, |chunk| {
                chunk[id as usize % CHUNK_IDS].load(Ordering::Relaxed)
            })
    }
}

impl IdCounts {
    /// ID `id` の、rc が正の生きている物体の数。
    pub(crate) fn owned_by(&self, id: u32) -> u64 {
        self.control
            .as_ref()
            .map_or(0, |control| control.workers.live(id))
    }
}

impl Control {
    /// ID ごとの数を、`heaps` のページの rc が正の生きている物体の所有者から数え直して確かめる。食い違う ID のうち最小の
    /// ものを `OwnedMiscount` で返す。速いモードでは何もしない (docs/spec/object-model.md の「論理的な worker ID」)。
    ///
    /// 数え直す前に、この `Control` のすべてのヒープを渡し、どのヒープもほかのスレッドからの解放を取り込み終えていて、
    /// 呼んでいる間どのスレッドもヒープに触れないこと。この関数は取り込めないので、検査モードでは、受けて取り込んで
    /// いない解放の残るヒープがあれば `NotQuiescent` を返す。
    ///
    /// # Panics
    /// ほかの `Control` のヒープを渡したとき。
    pub fn check_owned<M: Mode>(&self, heaps: &[&Heap<M>]) -> Result<(), HeapError> {
        assert!(
            heaps.iter().all(|heap| ptr::eq(&**heap.control(), self)),
            "check_owned over a heap of another Control"
        );
        if !M::CHECKED {
            return Ok(());
        }
        if heaps.iter().any(|heap| heap.remote_pending()) {
            return Err(HeapError::NotQuiescent);
        }
        let mut counted: BTreeMap<u32, u64> = BTreeMap::new();
        for heap in heaps {
            for owner in heap.owned_entries() {
                *counted.entry(owner).or_default() += 1;
            }
        }
        let chunks = lock(&self.workers.chunks);
        let table = chunks.len() * CHUNK_IDS;
        let in_table = (0..table).find(|&i| {
            chunks[i / CHUNK_IDS][i % CHUNK_IDS].load(Ordering::Relaxed)
                != counted.get(&(i as u32)).copied().unwrap_or(0)
        });
        let past_table = counted
            .keys()
            .map(|&id| id as usize)
            .find(|&id| id >= table);
        match in_table.into_iter().chain(past_table).min() {
            Some(worker) => Err(HeapError::OwnedMiscount {
                worker: worker as u32,
            }),
            None => Ok(()),
        }
    }
}

/// 止める理由。割り込み、終了の求め (`TERMINATE`)、確保量、知らせ、切り替え、深さの順に先に報告する。ビットがどれも立っていなければ、
/// 深さの上限で止めたとする。ビットは呼ぶ側が速い道で読んだ値を受け、読み直さない。読み直すと、その間にほかのスレッドが `clear`
/// したとき、深さの上限を超えていなくても `DepthLimit` を返すためである。
#[cold]
pub(crate) fn stop_reason(bits: u32) -> Stop {
    if bits & Control::INTERRUPT != 0 {
        Stop::Interrupted
    } else if bits & Control::TERMINATE != 0 {
        Stop::Terminated
    } else if bits & Control::MEMORY != 0 {
        Stop::MemoryLimit
    } else if bits & Control::NOTICE != 0 {
        Stop::Notice
    } else if bits & Control::PREEMPT != 0 {
        Stop::Preempt
    } else {
        Stop::DepthLimit
    }
}
