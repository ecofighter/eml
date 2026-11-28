//! workerのループが動かすもの。エンジンが入る項である (docs/spec/runtime.mdの「もの」)。

use eml_rt::Word;

use crate::cells::CellRef;

/// 根の項。worker 0が動かす。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    /// `main`。runの最初の項である。
    Main,
    /// 根のスコープが閉じた後の項。`main`が根のスコープを置いたrunの最後の項である。
    Result,
}

/// タスクの項の始め方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// タスクを始める。項の語はタスクの閉包である。
    Start,
    /// 待っていたタスクを`wake`で再開する。項の語は継続である。
    Resume { wake: i64 },
    /// driverの操作を待っていたタスクを、`wake`と記録の番号`op`で再開する。項の語は継続である。
    ResumeIo { wake: i64, op: i64 },
}

/// タスクの項。`worker`はタスクの論理的なIDで、項はその下で動く。`word`は項が参照を1つ持つ語である。
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub cell: CellRef,
    pub worker: u32,
    pub kind: EntryKind,
    pub word: Word,
}

/// `Exec::enter`が終わりか次の待ちまで動かす、1つの項。
#[derive(Debug, PartialEq, Eq)]
pub enum Item {
    Root(Root),
    Entry(Entry),
}

/// workerの間を移る語。作り方はどれも、語を渡せる形にした`share`を名指す (docs/spec/runtime.mdの「信頼の一覧」の
/// SM-6)。
pub struct Marked(Word);

impl Marked {
    pub fn into_word(self) -> Word {
        self.0
    }
}
