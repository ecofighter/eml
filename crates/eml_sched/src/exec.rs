//! エンジンがrunの手順に渡すもの。自分のworkerのfactoryと、項を動かすものである (docs/spec/runtime.mdの「runの手順」
//! 「信頼の一覧」)。

use std::sync::Arc;

use eml_rt::{Control, Ctx, HostRef, Mode, Owners};

use crate::core::WorkerCore;
use crate::job::Item;
use crate::policy::Policy;

/// workerが止まる理由。`Stopped`は、ヒープが操作の間にあることを表す。`Panicked`は、ヒープが操作の途中にあるかもしれ
/// ないことを表すので、受け取る側はヒープを切り離し、その中の何もたどらず、確かめない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Halt {
    Stopped,
    Panicked,
}

/// workerが動く場所。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// worker `k`。止める語と欄が`k`番目である。
    Worker(u32),
}

/// `sequence::run`が1つのworkerのfactoryに渡すもの。runの共有の部分と、1回だけ取り出せるworkerの核である。
pub struct Env<'e, P> {
    place: Place,
    control: &'e Arc<Control>,
    owners: &'e Owners,
    host: &'e HostRef,
    core: Option<WorkerCore<P>>,
}

impl<'e, P> Env<'e, P> {
    pub(crate) fn new(
        place: Place,
        control: &'e Arc<Control>,
        owners: &'e Owners,
        host: &'e HostRef,
        core: WorkerCore<P>,
    ) -> Env<'e, P> {
        Env {
            place,
            control,
            owners,
            host,
            core: Some(core),
        }
    }

    pub fn place(&self) -> Place {
        self.place
    }

    pub fn control(&self) -> &'e Arc<Control> {
        self.control
    }

    /// runのすべての`Ctx`が数える所有者の表。
    pub fn owners(&self) -> &'e Owners {
        self.owners
    }

    pub fn host(&self) -> &'e HostRef {
        self.host
    }

    /// workerの核。2回目からは`None`を返す。
    pub fn take_core(&mut self) -> Option<WorkerCore<P>> {
        self.core.take()
    }
}

/// 1つのworkerの項を動かすもの。値はfactoryのフレームの中にあり、ループはrunのcallbackが受け取ったポインタを通してだ
/// けそれに届く。
///
/// 実装する側の約束 (docs/spec/runtime.mdの「信頼の一覧」のSM-5)。`with`は`f`に、このworkerのスレッドの核と`Ctx`を貸
/// し、`f`が戻った後はどちらの借用も持たない。`enter`はその`Ctx`の上で項を1つ、終わりか次の待ちまで動かし、ヒープが操
/// 作の途中にあるかもしれないところでpanicを捕まえたときは`Halt::Panicked`を返す。
pub trait Exec {
    type M: Mode;
    type P: Policy;

    /// 項の間に、核と`Ctx`を`f`に貸す。
    ///
    /// # Safety
    /// `this`は、workerの自分のスレッドでrunのcallbackが受け取ったポインタで、それから導いた参照は呼び出しをまたいで
    /// 生きていない。
    unsafe fn with<R>(
        this: *mut Self,
        f: impl FnOnce(&mut WorkerCore<Self::P>, &mut Ctx<Self::M>) -> R,
    ) -> R;

    /// `item`を終わりか次の待ちまで動かす。
    ///
    /// # Safety
    /// `with`と同じ。呼ぶ側は呼び出しをまたいで、`this`から導いたものを何も持たない。
    unsafe fn enter(this: *mut Self, item: Item) -> Result<(), Halt>;
}

/// runのworkerのfactory。`sequence::run`は、各workerの自分のスレッドで`on_worker`を1回呼ぶ。
///
/// 実装する側の約束 (docs/spec/runtime.mdの「信頼の一覧」のSM-9)。`on_worker`は`run`を多くても1回呼ぶ。渡すポインタの
/// 先の`Exec`は、`env`から取った核と、このスレッドで`env.owners()`の上に止める語`k`で`Ctx::for_worker`が作った`Ctx`を
/// 持つ。最初の項の前に`Control`に対して確保するものは、どれも`run`を呼ぶ前に確保する。`run`の外でpanicせず (`policy`
/// も)、フレームの巻き戻しが`Ctx`を落とさずにリークするように、たとえば普通に戻るまで`ManuallyDrop`に入れて持つ。核を
/// このスレッドで落とし、`Ctx`を切り離して落とさずに持つ`Retired`を返す。`run`が`Err(Halt::Panicked)`を返したときは、
/// `Ctx`を切り離して包むだけにする。それでも`on_worker`が巻き戻ったら、`sequence::run`はプロセスを中止する。
pub trait Workers: Sync {
    type Exec: Exec;
    /// workerのスレッドが終わるときに残すもの。切り離した`Ctx`である。`Send`で、核を持たない。
    type Retired: Send;

    /// `place`のworkerの方針。そのworkerのスレッドで作る。
    fn policy(&self, place: Place) -> <Self::Exec as Exec>::P;

    /// `env`のworkerを用意し、その`Exec`で`run`を呼び、退かせるものを返す。
    ///
    /// # Safety
    /// `sequence::run`だけが、workerごとに1回、そのworkerのスレッドで、自分が組んだ`Env`を渡して呼ぶ。
    unsafe fn on_worker(
        &self,
        env: Env<'_, <Self::Exec as Exec>::P>,
        run: impl FnOnce(*mut Self::Exec) -> Result<(), Halt>,
    ) -> Self::Retired;
}
