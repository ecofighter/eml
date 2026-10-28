//! 実行時エラー (docs/spec/lir.md の「実行時エラー」)。表示は CLI と UI テストが使う文言である。`eml_cli` はこの型を
//! 再公開する (docs/spec/vm.md の「置き場所」)。

use std::fmt;

use eml_rt::{ArithFault, Fault, HeapError, Stop};

use crate::cells::CellFault;

#[derive(Debug, PartialEq, Eq)]
pub enum VmFault {
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
}

/// デッドロックの文言。関数の名前と位置は内部のコードとスケジュールで変わるので付けない。
const DEADLOCK: &str = "deadlock: every task is waiting and no timer is pending";

impl fmt::Display for VmFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmFault::Extern(fault) => write!(f, "{fault}"),
            VmFault::Panicked(message) => write!(f, "panicked: {message}"),
            VmFault::Stopped(stop) => write!(f, "{stop}"),
            VmFault::Internal(what) => write!(f, "internal error: {what}"),
            VmFault::Deadlock => f.write_str(DEADLOCK),
        }
    }
}

impl From<Fault> for VmFault {
    fn from(fault: Fault) -> VmFault {
        VmFault::Extern(fault)
    }
}

impl From<HeapError> for VmFault {
    fn from(error: HeapError) -> VmFault {
        VmFault::Extern(Fault::Heap(error))
    }
}

impl From<ArithFault> for VmFault {
    fn from(fault: ArithFault) -> VmFault {
        VmFault::Extern(Fault::Arith(fault))
    }
}

impl From<CellFault> for VmFault {
    fn from(fault: CellFault) -> VmFault {
        match fault {
            CellFault::Heap(error) => error.into(),
            CellFault::Internal(what) => VmFault::Internal(what),
        }
    }
}

impl From<Stop> for VmFault {
    fn from(stop: Stop) -> VmFault {
        VmFault::Stopped(stop)
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
        fault: VmFault,
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
                fault: VmFault::Stopped(Stop::Interrupted)
                    | VmFault::Extern(Fault::Stopped(Stop::Interrupted)),
                ..
            }
        )
    }

    /// 続けて実行できない誤りか。ヒープの誤り (根の誤った読み書き `RootMisuse` を含む) と内部の誤りの後と、回復が
    /// 失敗した後は、ヒープを信用できない。REPL はこの後の入力を受け付けない (docs/spec/vm.md の「追記」)。
    pub fn is_broken(&self) -> bool {
        matches!(
            self,
            RuntimeError::Fault {
                fault: VmFault::Internal(_) | VmFault::Extern(Fault::Heap(_)),
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
