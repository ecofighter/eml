//! 実行時エラー (docs/spec/lir.md の「実行時エラー」)。表示は CLI と UI テストが使う文言である。`eml_cli` はこの型を
//! 再公開する (docs/spec/vm.md の「置き場所」)。

use std::fmt;

use eml_rt::{ArithFault, Fault, HeapError, Stop};

#[derive(Debug, PartialEq, Eq)]
pub enum VmFault {
    /// extern、基本操作、ヒープの操作の誤り。
    Extern(Fault),
    /// 安全点で、割り込みか上限で止めた。
    Stopped(Stop),
    /// verifier を通った IR では起きないはずの誤り。
    Internal(&'static str),
}

impl fmt::Display for VmFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmFault::Extern(fault) => write!(f, "{fault}"),
            VmFault::Stopped(stop) => write!(f, "{stop}"),
            VmFault::Internal(what) => write!(f, "internal error: {what}"),
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
}

impl RuntimeError {
    /// 割り込みで止まったか。安全点で止めたときと、`Fs.read_all` の中で止めたときの両方で真になる。CLI が終了コードを
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

    /// 続けて実行できない誤りか。ヒープの誤り (根の誤った読み書き `RootMisuse` を含む) と内部の誤りの後は、ヒープを
    /// 信用できない。REPL はこの後の入力を受け付けない (docs/spec/vm.md の「追記」)。
    pub fn is_broken(&self) -> bool {
        matches!(
            self,
            RuntimeError::Fault {
                fault: VmFault::Internal(_) | VmFault::Extern(Fault::Heap(_)),
                ..
            }
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
        }
    }
}

impl std::error::Error for RuntimeError {}
