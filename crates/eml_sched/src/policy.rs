//! スケジュールの方針と、どのインタプリタも知らせるフック (docs/spec/runtime.mdの「フック」)。

use eml_steps::Label;

/// workerの仕事の地点。`Policy::at`は、その地点を行う前にそれを見る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Point {
    /// `eml_steps`のカーネルの手順か効果。それを行うインタプリタが知らせる。
    Kernel(Label),
    Shell(ShellPoint),
}

/// どのカーネルも決めない、シェルの地点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellPoint {
    /// `wait_join`が待つ前。
    JoinWait,
    /// hostが操作を自分のdriverに出す前。
    Submit,
    /// hostが完了を持ち主に渡すために`cells`を呼ぶ前。
    HandBack,
    /// workerの最後の`wake_all`の前。
    Leave,
}

/// workerの方針。スケジューラが読むものを何も決めない。スケジューラは地点を行う前に、lockを持たずに`at`を呼び、何も読
/// み返さない。テストは`at`の中で止まって、地点の順を決める。
pub trait Policy {
    fn at(&self, _worker: u32, _point: Point) {}
}

/// `Sched::Normal`と`Sched::Sequential`の方針。何も見ない。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InOrder;

impl Policy for InOrder {}
