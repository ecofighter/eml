//! 実行時エラー。表示は CEK (`eml_interp::RuntimeError`) と1文字も違わない形にする。S10b で CEK を消すときに、
//! `eml_cli` が公開する誤りの型をこちらに替える (docs/spec/vm.md の「置き場所」)。

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
pub enum VmError {
    Fault {
        fault: VmFault,
        function: String,
        at: Option<SourceLocation>,
    },
    /// 検査モードで、終わったときに残っていた物体の種類の名前と数。
    Leak(Vec<(String, u64)>),
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VmError::Fault {
                fault,
                function: _,
                at: Some(at),
            } => write!(f, "{fault}\n  at {}:{}:{}", at.path, at.line, at.column),
            VmError::Fault {
                fault,
                function,
                at: None,
            } => write!(f, "{fault} in `{function}`"),
            VmError::Leak(live) => {
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

impl std::error::Error for VmError {}
