//! Rustの閉包でできたエンジン。テストはこれを通してスケジューラを動かす (docs/spec/runtime.mdの「範囲」)。

use std::mem::ManuallyDrop;
use std::sync::Arc;

use eml_rt::{Ctx, Mode};

use crate::core::WorkerCore;
use crate::exec::{Env, Exec, Halt, Place, Workers};
use crate::job::Item;
use crate::policy::Policy;
use crate::rank::Ranked;

/// workerの項を動かす閉包。workerの核、`Ctx`、項を受け取る。
pub type Enter<'a, M, P> =
    dyn Fn(&mut WorkerCore<P>, &mut Ctx<M>, Item) -> Result<(), Halt> + Send + Sync + 'a;

/// `enter`が閉包を呼ぶ`Exec`。
pub struct Script<'a, M: Mode + 'a, P: Policy + 'a> {
    core: WorkerCore<P>,
    ctx: Held<'a, M>,
    enter: Arc<Enter<'a, M, P>>,
}

/// scriptの`Ctx`。
#[expect(
    clippy::large_enum_variant,
    reason = "a worker has one script, made once per run"
)]
enum Held<'a, M: Mode> {
    // scriptを持つフレームの巻き戻しは`Ctx`をリークする (SM-9)
    Made(ManuallyDrop<Ctx<M>>),
    /// 呼ぶスレッドが貸した、runより長く生きる`Ctx`。
    Lent(&'a mut Ctx<M>),
}

impl<M: Mode> Held<'_, M> {
    fn ctx(&mut self) -> &mut Ctx<M> {
        match self {
            Held::Made(ctx) => ctx,
            Held::Lent(ctx) => ctx,
        }
    }
}

impl<'a, M: Mode + 'a, P: Policy + 'a> Script<'a, M, P> {
    pub fn new(core: WorkerCore<P>, ctx: Ctx<M>, enter: Arc<Enter<'a, M, P>>) -> Script<'a, M, P> {
        Script {
            core,
            ctx: Held::Made(ManuallyDrop::new(ctx)),
            enter,
        }
    }
}

impl<'a, M: Mode + 'a, P: Policy + 'a> Exec for Script<'a, M, P> {
    type M = M;
    type P = P;

    unsafe fn with<R>(this: *mut Self, f: impl FnOnce(&mut WorkerCore<P>, &mut Ctx<M>) -> R) -> R {
        // SAFETY: `Exec::with`の`# Safety`により、`this`は有効で、呼び出しの間ほかに借りるものはない。
        let this = unsafe { &mut *this };
        f(&mut this.core, this.ctx.ctx())
    }

    unsafe fn enter(this: *mut Self, item: Item) -> Result<(), Halt> {
        // SAFETY: `Exec::enter`の`# Safety`による。`with`と同じである。
        let this = unsafe { &mut *this };
        (this.enter)(&mut this.core, this.ctx.ctx(), item)
    }
}

/// workerごとに閉包を1つ持つ`Workers`。各workerの`Ctx`はそのスレッドで`Ctx::for_worker`で作り、`Retired`として戻る。
/// `lending`で作ったものは、worker 0に呼ぶスレッドの`Ctx`を貸し、その`Retired`は`None`である。何も捕まえない。panicし
/// た閉包はrunのcallbackまで巻き戻り、callbackがそれを記録する。
pub struct Scripts<'a, M: Mode + 'a, P: Policy + 'a> {
    policy: Box<dyn Fn(Place) -> P + Sync + 'a>,
    scripts: Vec<Arc<Enter<'a, M, P>>>,
    /// 貸す`Ctx`。最初のrunのworker 0が取る。
    lent: Option<Ranked<Option<&'a mut Ctx<M>>>>,
}

impl<'a, M: Mode + 'a, P: Policy + 'a> Scripts<'a, M, P> {
    /// worker `k`は、`policy`がそのworkerのために作る方針の下で、`scripts[k]`で項を動かす。
    pub fn new(
        policy: impl Fn(Place) -> P + Sync + 'a,
        scripts: Vec<Box<Enter<'a, M, P>>>,
    ) -> Scripts<'a, M, P> {
        Scripts {
            policy: Box::new(policy),
            scripts: scripts.into_iter().map(Arc::from).collect(),
            lent: None,
        }
    }

    /// `new`と同じだが、worker 0は`ctx`の上で項を動かす。`session()`は真で、runはworkerを1つだけ持つ。`ctx`を貸すの
    /// は、これを渡す最初のrunだけである。止まったrunの後、貸す側は、もう一度貸す前に`ctx`を回復するか落とす。worker 0で
    /// `Halt::Panicked`があった後の`ctx`は、貸す側が落とさず、たどらず、回復しないものである。
    ///
    /// # Safety
    /// `ctx`は呼ぶスレッドの`Ctx`で、これを渡すrunは呼ぶスレッドで動き、`Start.owners`は`ctx`のヒープの表である。
    pub unsafe fn lending(
        ctx: &'a mut Ctx<M>,
        policy: impl Fn(Place) -> P + Sync + 'a,
        scripts: Vec<Box<Enter<'a, M, P>>>,
    ) -> Scripts<'a, M, P> {
        Scripts {
            lent: Some(Ranked::new(Some(ctx))),
            ..Scripts::new(policy, scripts)
        }
    }
}

impl<'a, M: Mode + 'a, P: Policy + 'a> Workers for Scripts<'a, M, P> {
    type Exec = Script<'a, M, P>;
    type Retired = Option<Ctx<M>>;

    fn policy(&self, place: Place) -> P {
        (self.policy)(place)
    }

    fn session(&self) -> bool {
        self.lent.is_some()
    }

    unsafe fn on_worker(
        &self,
        mut env: Env<'_, P>,
        run: impl FnOnce(*mut Script<'a, M, P>) -> Result<(), Halt>,
    ) -> Option<Ctx<M>> {
        let Place::Worker(k) = env.place();
        let enter = self.scripts[k as usize].clone();
        let core = env.take_core().expect("the core is taken once");
        if let Some(lent) = &self.lent {
            // 貸す`Ctx`は1回のrunにだけ貸す。2回目のrunのfactoryは`run`を呼ばずに戻る
            let taken = lent.hold().take();
            if let Some(ctx) = taken {
                let mut script = Script {
                    core,
                    ctx: Held::Lent(ctx),
                    enter,
                };
                let _ = run(&mut script);
            }
            return None;
        }
        // `Ctx`はそのままscriptの`ManuallyDrop`に入るので、後のpanicがそれを落とすことはない
        let ctx = Ctx::for_worker(
            env.control().clone(),
            env.owners(),
            env.host().clone(),
            k as usize,
        );
        let mut script = Script::new(core, ctx, enter);
        let _ = run(&mut script);
        let Script { core, ctx, enter } = script;
        // フレームのほかのものを先に落とすので、`Ctx`が`ManuallyDrop`の外にある間はどの落とし方も動かない
        drop(core);
        drop(enter);
        drop(env);
        match ctx {
            Held::Made(ctx) => Some(ManuallyDrop::into_inner(ctx)),
            Held::Lent(_) => None,
        }
    }
}
