//! 実行時エラー (docs/spec/lir.md の「実行時エラー」)。表示は CLI と UI テストが使う文言である。`eml_cli` はこの型を
//! 再公開する (docs/spec/vm.md の「置き場所」)。

use std::fmt;

use eml_rt::{ArithFault, Fault, HeapError, Stop};

use crate::cells::CellFault;

#[derive(Debug, PartialEq, Eq)]
pub enum RunFault {
    /// extern、基本操作、ヒープの操作の誤り。
    Extern(Fault),
    /// 捕まえる `try_panic` のない利用者の `panic` と、その文言 (docs/spec/vm.md の「`IO` と `Panic` への中断」)。
    Panicked(String),
    /// 安全点で、割り込みか上限で止めた。
    Stopped(Stop),
    /// verifier を通った IR では起きないはずの誤り。
    Internal(&'static str),
    /// 実行待ちのタスクも時間の待ちもないのに、終わっていないタスクがある (docs/spec/vm.md の「時計と driver」)。
    Deadlock,
    /// I/O の driver を作れなかったか、driver が壊れた (docs/spec/vm.md の「時計と driver」)。
    Driver(String),
    /// メインのworkerの`Park`が、自分の待ちの誤りを返した (docs/spec/vm.mdの「メインのworkerの待ち」)。
    Park(String),
}

/// `quiesce`の後にもdriverが保留の操作を残したときの、`RunFault::Driver`の文。そのdriverはどのヒープにも触れる前に
/// 落とす (docs/spec/vm.mdの「時計と driver」)。
pub const KEPT_PENDING: &str = "the driver kept pending ops after quiesce";

/// デッドロックの文言。関数の名前と位置は内部のコードとスケジュールで変わるので付けない。
const DEADLOCK: &str = "deadlock: every task is waiting and no timer is pending";

impl fmt::Display for RunFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunFault::Extern(fault) => write!(f, "{fault}"),
            RunFault::Panicked(message) => write!(f, "panicked: {message}"),
            RunFault::Stopped(stop) => write!(f, "{stop}"),
            RunFault::Internal(what) => write!(f, "internal error: {what}"),
            RunFault::Deadlock => f.write_str(DEADLOCK),
            RunFault::Driver(error) => write!(f, "the I/O driver failed: {error}"),
            RunFault::Park(error) => write!(f, "the main worker's park failed: {error}"),
        }
    }
}

impl From<Fault> for RunFault {
    fn from(fault: Fault) -> RunFault {
        RunFault::Extern(fault)
    }
}

impl From<HeapError> for RunFault {
    fn from(error: HeapError) -> RunFault {
        RunFault::Extern(Fault::Heap(error))
    }
}

impl From<ArithFault> for RunFault {
    fn from(fault: ArithFault) -> RunFault {
        RunFault::Extern(Fault::Arith(fault))
    }
}

impl From<CellFault> for RunFault {
    fn from(fault: CellFault) -> RunFault {
        match fault {
            CellFault::Heap(error) => error.into(),
            CellFault::Internal(what) => RunFault::Internal(what),
        }
    }
}

impl From<Stop> for RunFault {
    fn from(stop: Stop) -> RunFault {
        RunFault::Stopped(stop)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    pub path: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeError {
    Fault {
        fault: RunFault,
        function: String,
        at: Option<SourceLocation>,
    },
    /// 検査モードで、終わったときに残っていた物体の種類の名前と数。
    Leak(Vec<(String, u64)>),
    /// すべてのタスクが待ち、満ちるタイマーもない。関数の名前も位置も持たない。
    Deadlock,
    /// 続けられる誤り `cause` の後の回復が `reason` で失敗した。根から届く物体を写せなかったか、検査モードで回復の後の
    /// ヒープの不変条件が崩れていた (docs/spec/vm.md の「追記」)。
    Unrecovered {
        cause: Box<RuntimeError>,
        reason: HeapError,
    },
}

impl RuntimeError {
    /// 割り込みで止まったか。安全点で止めたときと、ハンドルの読みの中で止めたときの両方で真になる。CLI が終了コードを
    /// 決めるために使う。
    pub fn interrupted(&self) -> bool {
        matches!(
            self,
            RuntimeError::Fault {
                fault: RunFault::Stopped(Stop::Interrupted)
                    | RunFault::Extern(Fault::Stopped(Stop::Interrupted)),
                ..
            }
        )
    }

    /// 終了の求め (unix の SIGTERM と、Windows のコンソールを閉じることとシャットダウン) で止まったか。
    /// `interrupted` と同じく、安全点で止めたときと、ハンドルの読みの中で止めたときの両方で真になる。CLI が終了コードを決めるために使う。
    pub fn terminated(&self) -> bool {
        matches!(
            self,
            RuntimeError::Fault {
                fault: RunFault::Stopped(Stop::Terminated)
                    | RunFault::Extern(Fault::Stopped(Stop::Terminated)),
                ..
            }
        )
    }

    /// 続けて実行できない誤りか。ヒープの誤り (根の誤った読み書き`RootMisuse`を含む) と内部の誤りと、中身をエンジンが
    /// 持つ失敗`Fault::Engine`の後と、回復が失敗した後は、ヒープを信用できない。driverの誤りの後は、その`Machine`の
    /// I/Oを信用することも続けることもできない。
    /// driverを作れなかったときも、その`Machine`ではI/Oを動かせないので同じに扱う。REPLはこの後の入力を受け付けない
    /// (docs/spec/vm.mdの「追記」と「時計と driver」)。
    pub fn is_broken(&self) -> bool {
        matches!(
            self,
            RuntimeError::Fault {
                fault: RunFault::Internal(_)
                    | RunFault::Driver(_)
                    | RunFault::Extern(Fault::Heap(_) | Fault::Engine),
                ..
            } | RuntimeError::Unrecovered { .. }
        )
    }
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeError::Fault {
                fault,
                function: _,
                at: Some(at),
            } => write!(f, "{fault}\n  at {}:{}:{}", at.path, at.line, at.column),
            RuntimeError::Fault {
                fault,
                function,
                at: None,
            } => write!(f, "{fault} in `{function}`"),
            RuntimeError::Leak(live) => {
                let parts: Vec<String> =
                    live.iter().map(|(name, n)| format!("{n} {name}")).collect();
                write!(
                    f,
                    "memory leak: objects were not freed: {}",
                    parts.join(", ")
                )
            }
            RuntimeError::Deadlock => f.write_str(DEADLOCK),
            RuntimeError::Unrecovered { cause, reason } => {
                write!(f, "{cause}\nrecovery failed: {reason}")
            }
        }
    }
}

impl std::error::Error for RuntimeError {}
