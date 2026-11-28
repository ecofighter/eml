//! workerのループが動かすもの。エンジンが入る項である (docs/spec/runtime.mdの「もの」)。

/// 根の項。worker 0が動かす。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    /// `main`。runの最初の項である。
    Main,
}

/// `Exec::enter`が終わりか次の待ちまで動かす、1つの項。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Root(Root),
}
