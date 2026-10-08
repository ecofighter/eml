use std::fmt;

use eml_runtime::HeapError;

/// 実行時エラー (docs/spec/core-ir.md の「実行時エラー」)。表示は CLI と UI テストが使う文言である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// 実行中の関数で止まった。`at` は、位置を持つ extern の呼び出しが起こした誤りにだけ付く。
    Fault {
        fault: Fault,
        function: String,
        at: Option<SourceLocation>,
    },
    /// `debug_heap` で、終了時に解放されていないオブジェクトがあった。オブジェクトの種類の名前ごとの数。
    Leak(Vec<(String, usize)>),
}

/// 実行時エラーを起こしたソースの位置。`column` は1から数える文字の位置である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// 表示用のパス。
    pub path: String,
    pub line: u32,
    pub column: u32,
}

/// 実行中の関数で起きた誤り。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    DivisionByZero,
    IntegerOverflow,
    Heap(HeapError),
    Output(String),
    /// `open` がファイルを開けなかった。理由は `ErrorKind` から決めた固定の文言で、OS の文言は環境ごとに違うので使わない。
    FileOpen {
        path: String,
        reason: &'static str,
    },
    FileRead {
        path: String,
        reason: &'static str,
    },
    FileNotUtf8 {
        path: String,
    },
    /// 型検査と Core IR の変換が正しければ起きない誤り。
    Internal(&'static str),
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::DivisionByZero => f.write_str("division by zero"),
            Fault::IntegerOverflow => f.write_str("integer overflow"),
            Fault::Heap(error) => write!(f, "{error}"),
            Fault::Output(error) => write!(f, "cannot write the output: {error}"),
            Fault::FileOpen { path, reason } => write!(f, "cannot open `{path}`: {reason}"),
            Fault::FileRead { path, reason } => write!(f, "cannot read `{path}`: {reason}"),
            Fault::FileNotUtf8 { path } => write!(f, "`{path}` is not valid UTF-8"),
            Fault::Internal(what) => write!(f, "internal error: {what}"),
        }
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

pub(crate) fn io_reason(kind: std::io::ErrorKind) -> &'static str {
    match kind {
        std::io::ErrorKind::NotFound => "not found",
        std::io::ErrorKind::PermissionDenied => "permission denied",
        _ => "I/O error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_errors_have_fixed_reasons() {
        // OS の文言は環境ごとに違うので、実行時エラーの文言には `ErrorKind` から決めた理由だけを入れる
        assert_eq!(io_reason(std::io::ErrorKind::NotFound), "not found");
        assert_eq!(
            io_reason(std::io::ErrorKind::PermissionDenied),
            "permission denied"
        );
        assert_eq!(io_reason(std::io::ErrorKind::IsADirectory), "I/O error");
    }

    #[test]
    fn runtime_errors_name_the_fault_and_the_function() {
        let fault = |fault| {
            RuntimeError::Fault {
                fault,
                function: "f".to_string(),
                at: None,
            }
            .to_string()
        };
        assert_eq!(fault(Fault::DivisionByZero), "division by zero in `f`");
        assert_eq!(fault(Fault::IntegerOverflow), "integer overflow in `f`");
        assert_eq!(
            fault(Fault::Heap(HeapError::UseAfterFree)),
            "use of a freed object in `f`"
        );
        assert_eq!(
            fault(Fault::Output("broken pipe".to_string())),
            "cannot write the output: broken pipe in `f`"
        );
        assert_eq!(
            fault(Fault::Internal("a switch without a matching case")),
            "internal error: a switch without a matching case in `f`"
        );
    }

    #[test]
    fn a_runtime_error_with_a_location_names_the_place_instead_of_the_function() {
        // 位置があれば関数の名前を出さない。値として使う extern を包む関数の名前 (`main$extern0`) より、呼び出した場所の
        // ほうが役に立つためである (docs/spec/core-ir.md の「実行時エラー」)
        let error = RuntimeError::Fault {
            fault: Fault::DivisionByZero,
            function: "main$extern0".to_string(),
            at: Some(SourceLocation {
                path: "main.em".to_string(),
                line: 2,
                column: 20,
            }),
        };
        assert_eq!(error.to_string(), "division by zero\n  at main.em:2:20");
    }

    #[test]
    fn leaks_are_displayed_with_the_object_counts() {
        let leak = RuntimeError::Leak(vec![("Closure".to_string(), 2), ("String".to_string(), 1)]);
        assert_eq!(
            leak.to_string(),
            "memory leak: objects were not freed: 2 Closure, 1 String"
        );
    }
}
