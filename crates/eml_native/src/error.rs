//! つなぐ失敗。文言は固定で、OS の文言を含まない (docs/spec/vm.md の「ライブラリと記号」)。

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkError {
    Library {
        name: String,
    },
    /// `library` は探したライブラリの名前を引用して `", "` でつないだもの。探したライブラリがなければ `None`。
    Symbol {
        symbol: String,
        library: Option<String>,
    },
    UnknownOverride {
        key: String,
    },
    Unsupported {
        reason: String,
    },
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::Library { name } => write!(f, "library \"{name}\" was not found"),
            LinkError::Symbol { symbol, library } => {
                write!(f, "symbol \"{symbol}\" was not found")?;
                if let Some(library) = library {
                    write!(f, " in {library}")?;
                }
                Ok(())
            }
            LinkError::UnknownOverride { key } => write!(f, "--link names no library \"{key}\""),
            LinkError::Unsupported { reason } => {
                write!(f, "C functions cannot be called on this machine: {reason}")
            }
        }
    }
}

impl std::error::Error for LinkError {}
