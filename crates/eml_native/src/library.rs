//! ライブラリの名前の飾りと、開いたライブラリ (docs/spec/vm.md の「ライブラリと記号」)。

#![allow(unsafe_code)]

use std::collections::HashMap;
use std::mem::ManuallyDrop;

use eml_extern::{CLibrary, Os};

/// 動いている OS。C の関数を呼べない OS では `None`。
pub fn running_os() -> Option<Os> {
    if cfg!(target_os = "linux") {
        Some(Os::Linux)
    } else if cfg!(target_os = "macos") {
        Some(Os::Macos)
    } else if cfg!(windows) {
        Some(Os::Windows)
    } else {
        None
    }
}

/// ローダーに渡す道の候補を、試す順に返す。
pub fn candidates(os: Os, library: &CLibrary) -> Vec<String> {
    match library {
        CLibrary::Named(name) if name.contains(['/', '\\', '.']) => vec![name.clone()],
        CLibrary::Named(name) => vec![match os {
            Os::Linux => format!("lib{name}.so"),
            Os::Macos => format!("lib{name}.dylib"),
            Os::Windows => format!("{name}.dll"),
        }],
        // macOS の dlopen は framework の相対の道を開けないので、絶対の道を並べる
        CLibrary::Framework(name) => vec![
            format!("/System/Library/Frameworks/{name}.framework/{name}"),
            format!("/Library/Frameworks/{name}.framework/{name}"),
        ],
    }
}

/// 開いたライブラリ。どれも最初に使うときに開き、閉じない。閉じると、そのライブラリから引いた `CCall` と `Dtor` の
/// ポインタが、解放したコードを指すことになるためである。
#[derive(Default)]
pub(crate) struct Libraries {
    open: HashMap<String, ManuallyDrop<libloading::Library>>,
    process: Vec<ManuallyDrop<libloading::Library>>,
    process_opened: bool,
}

impl Libraries {
    /// ライブラリを開く。どの候補も開けなければ `false`。鍵は最初の候補で、同じライブラリは一度だけ開く。
    pub(crate) fn open(&mut self, os: Os, library: &CLibrary) -> bool {
        let paths = candidates(os, library);
        if self.open.contains_key(&paths[0]) {
            return true;
        }
        for path in &paths {
            // SAFETY: ライブラリの初期化のコードが走る。どのライブラリを開くかは利用者が `link` と `--link` で選び、
            // その中身を信頼する (docs/spec/vm.md の「ライブラリと記号」)
            if let Ok(opened) = unsafe { libloading::Library::new(path) } {
                self.open
                    .insert(paths[0].clone(), ManuallyDrop::new(opened));
                return true;
            }
        }
        false
    }

    /// 開いたライブラリから記号を引く。`open` が `true` を返したライブラリだけに使う。
    pub(crate) fn find(&self, os: Os, library: &CLibrary, symbol: &str) -> Option<*const ()> {
        let paths = candidates(os, library);
        lookup(&self.open[&paths[0]], symbol)
    }

    /// プロセスの範囲から記号を引く。
    pub(crate) fn find_in_process(&mut self, symbol: &str) -> Option<*const ()> {
        if !self.process_opened {
            self.process_opened = true;
            self.process = process_libraries()
                .into_iter()
                .map(ManuallyDrop::new)
                .collect();
        }
        self.process
            .iter()
            .find_map(|library| lookup(library, symbol))
    }
}

fn lookup(library: &libloading::Library, symbol: &str) -> Option<*const ()> {
    // SAFETY: 記号のアドレスを読むだけで、型を当てて呼ぶのは橋渡しである。アドレスはライブラリを閉じない限り有効で、
    // `Libraries` はライブラリを閉じない
    let found = unsafe { library.get::<*const ()>(symbol.as_bytes()) }.ok()?;
    let address = *found;
    (!address.is_null()).then_some(address)
}

#[cfg(unix)]
fn process_libraries() -> Vec<libloading::Library> {
    vec![libloading::os::unix::Library::this().into()]
}

#[cfg(windows)]
fn process_libraries() -> Vec<libloading::Library> {
    // Windows のプロセスの範囲は実行ファイルの名前の表だけなので、C の標準ライブラリの `ucrtbase.dll` を足す
    let this = libloading::os::windows::Library::this().map(Into::into);
    // SAFETY: どの Windows にもある OS の C の標準ライブラリで、Rust の実行ファイルもこれを使う
    let ucrt = unsafe { libloading::Library::new("ucrtbase.dll") };
    [this, ucrt].into_iter().filter_map(Result::ok).collect()
}

#[cfg(not(any(unix, windows)))]
fn process_libraries() -> Vec<libloading::Library> {
    Vec::new()
}
