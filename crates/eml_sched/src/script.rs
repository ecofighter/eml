//! Rustの閉包でできたエンジン。テストはこれを通してスケジューラを動かす (docs/spec/runtime.mdの「範囲」)。

use std::mem::ManuallyDrop;
use std::sync::Arc;

use eml_rt::{Ctx, Mode};

use crate::core::WorkerCore;
use crate::exec::{Env, Exec, Halt, Place, Workers};
use crate::job::Item;
use crate::policy::Policy;

/// workerの項を動かす閉包。workerの核、`Ctx`、項を受け取る。
pub type Enter<'a, M, P> =
    dyn Fn(&mut WorkerCore<P>, &mut Ctx<M>, Item) -> Result<(), Halt> + Send + Sync + 'a;

/// `enter`が閉包を呼ぶ`Exec`。
pub struct Script<'a, M: Mode + 'a, P: Policy + 'a> {
    core: WorkerCore<P>,
    // scriptを持つフレームの巻き戻しは`Ctx`をリークする (SM-9)
    ctx: ManuallyDrop<Ctx<M>>,
    enter: Arc<Enter<'a, M, P>>,
}

impl<'a, M: Mode + 'a, P: Policy + 'a> Script<'a, M, P> {
    pub fn new(core: WorkerCore<P>, ctx: Ctx<M>, enter: Arc<Enter<'a, M, P>>) -> Script<'a, M, P> {
        Script {
            core,
            ctx: ManuallyDrop::new(ctx),
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
        f(&mut this.core, &mut this.ctx)
    }

    unsafe fn enter(this: *mut Self, item: Item) -> Result<(), Halt> {
        // SAFETY: `Exec::enter`の`# Safety`による。`with`と同じである。
        let this = unsafe { &mut *this };
        (this.enter)(&mut this.core, &mut this.ctx, item)
    }
}

/// workerごとに閉包を1つ持つ`Workers`。各workerの`Ctx`はそのスレッドで`Ctx::for_worker`で作り、`Retired`として戻る。
/// 何も捕まえない。panicした閉包はrunのcallbackまで巻き戻り、callbackがそれを記録する。
pub struct Scripts<'a, M: Mode + 'a, P: Policy + 'a> {
    policy: Box<dyn Fn(Place) -> P + Sync + 'a>,
    scripts: Vec<Arc<Enter<'a, M, P>>>,
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
        }
    }
}

impl<'a, M: Mode + 'a, P: Policy + 'a> Workers for Scripts<'a, M, P> {
    type Exec = Script<'a, M, P>;
    type Retired = Ctx<M>;

    fn policy(&self, place: Place) -> P {
        (self.policy)(place)
    }

    unsafe fn on_worker(
        &self,
        mut env: Env<'_, P>,
        run: impl FnOnce(*mut Script<'a, M, P>) -> Result<(), Halt>,
    ) -> Ctx<M> {
        let Place::Worker(k) = env.place();
        let enter = self.scripts[k as usize].clone();
        let core = env.take_core().expect("the core is taken once");
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
        ManuallyDrop::into_inner(ctx)
    }
}
