//! 後の並列 VM のための型の性質 (docs/future/multicore.md の「VM の並列化の備え」の制約1と2)。`CodeBase` は worker の
//! 間で共有し、`Task` は worker のスレッドへ移せる。

use eml_rt::{Checked, Fast};
use eml_vm::{CodeBase, Task};

fn sync<T: Sync>() {}

fn send<T: Send>() {}

#[test]
fn a_code_base_is_shared_and_a_task_moves() {
    sync::<CodeBase<Fast>>();
    sync::<CodeBase<Checked>>();
    send::<Task>();
}
