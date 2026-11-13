//! 記号の解決と `Linked` の組み立て (docs/spec/vm.md の「ライブラリと記号」)。

#![allow(unsafe_code)]

use std::collections::HashMap;

use eml_cabi::{CCall, Dtor, Linked};
use eml_extern::{CFunction, CHandleType, CLibrary, CLink, Os};

use crate::bridge::Bridges;
use crate::error::LinkError;
use crate::library::{Libraries, running_os};

/// 登録した記号のアドレス。
struct Address(*const ());

// SAFETY: アドレスを運ぶだけで、`Linker` はそれを参照外ししない。呼ぶのは `Linked` を受けた側で、指すコードは
// 登録した側がプロセスの間ずっと保つ
unsafe impl Send for Address {}

/// C の関数とハンドルの dtor をつなぐ。開いたライブラリと作った橋渡しを持ち続けるので、REPL は1つの `Linker` で
/// 入力ごとにつなぐ。
pub struct Linker {
    overrides: HashMap<String, String>,
    symbols: HashMap<String, Address>,
    libraries: Libraries,
    bridges: Bridges,
}

impl Linker {
    /// `overrides` は `--link KEY=VALUE` の対。何も開かず、JIT も作らない。
    pub fn new(overrides: Vec<(String, String)>) -> Linker {
        Linker {
            overrides: overrides.into_iter().collect(),
            symbols: HashMap::new(),
            libraries: Libraries::default(),
            bridges: Bridges::new(),
        }
    }

    /// どのライブラリより先に引く記号を登録する。テストが Rust の関数を C の関数として渡すために使う。
    ///
    /// # Safety
    /// どのアドレスも、その記号に解決するすべての宣言の C の署名を持つ関数である。dtor として引く記号のアドレスは
    /// `void f(void*)` として呼べる関数である。どれもプロセスが終わるまで有効である。ライブラリの記号と違い、登録した
    /// アドレスは宣言が保証しないので、この約束を登録する側が負う (docs/spec/vm.md の「ライブラリと記号」)。
    pub unsafe fn with_symbols(mut self, symbols: &[(&str, *const ())]) -> Linker {
        for &(name, address) in symbols {
            self.symbols.insert(name.to_owned(), Address(address));
        }
        self
    }

    /// 記号をすべて解決してから橋渡しを作り、入力に並びをそろえた `Linked` を返す。
    pub fn link(
        &mut self,
        functions: &[CFunction],
        handles: &[CHandleType],
    ) -> Result<Linked, LinkError> {
        if functions.is_empty() && handles.is_empty() {
            return Ok(Linked::default());
        }
        let os = running_os().ok_or_else(|| LinkError::Unsupported {
            reason: "the operating system is not Linux, macOS or Windows".to_owned(),
        })?;
        let mut callees = Vec::with_capacity(functions.len());
        for function in functions {
            callees.push(self.resolve(os, &function.symbol, &function.links)?);
        }
        let mut dtors = Vec::with_capacity(handles.len());
        for handle in handles {
            let dtor = match &handle.dtor {
                None => None,
                Some(symbol) => {
                    let address = self.resolve(os, symbol, &handle.links)?;
                    // SAFETY: dtor の C の署名は `void f(void*)` と宣言する側が約束する。登録した記号では
                    // `with_symbols` の呼び手が約束する。値を返す関数でも、戻り値を捨てる呼び出しは OS の既定の
                    // 呼び出し規約で正しい
                    Some(unsafe { std::mem::transmute::<*const (), Dtor>(address) })
                }
            };
            dtors.push((handle.clone(), dtor));
        }
        let signatures: Vec<_> = functions.iter().map(CFunction::c_signature).collect();
        let bridges = self.bridges.get(&signatures)?;
        let calls = functions
            .iter()
            .cloned()
            .zip(bridges.into_iter().zip(callees))
            .map(|(function, (bridge, callee))| (function, CCall { bridge, callee }))
            .collect();
        // SAFETY: 呼ばれる関数と dtor は、組にした宣言の記号と `links` を解決して得たもので、橋渡しはその宣言の
        // `c_signature()` で引いたものである。ライブラリの記号は宣言が境目として保証し
        // (docs/spec/declarations.md の「宣言する側の約束」)、登録した記号は `with_symbols` の呼び手が保証する。
        // 橋渡しのコードもライブラリもプロセスの間外さない (docs/spec/vm.md の「橋渡しの関数」、
        // docs/spec/vm.md の「ライブラリと記号」)
        Ok(unsafe { Linked::new(calls, dtors) })
    }

    fn resolve(&mut self, os: Os, symbol: &str, links: &[CLink]) -> Result<*const (), LinkError> {
        if let Some(Address(address)) = self.symbols.get(symbol) {
            return Ok(*address);
        }
        let libraries: Vec<CLibrary> = links
            .iter()
            .filter(|link| link.os == os)
            .map(|link| match self.overrides.get(link.library.written()) {
                Some(value) => CLibrary::Named(value.clone()),
                None => link.library.clone(),
            })
            .collect();
        // 動いている OS のライブラリは、記号がどこにあってもすべて開けなければならない
        for library in &libraries {
            if !self.libraries.open(os, library) {
                return Err(LinkError::Library {
                    name: library.written().to_owned(),
                });
            }
        }
        if let Some(address) = libraries
            .iter()
            .find_map(|library| self.libraries.find(os, library, symbol))
        {
            return Ok(address);
        }
        if let Some(address) = self.libraries.find_in_process(symbol) {
            return Ok(address);
        }
        let library = (!libraries.is_empty()).then(|| {
            libraries
                .iter()
                .map(|library| format!("\"{}\"", library.written()))
                .collect::<Vec<_>>()
                .join(", ")
        });
        Err(LinkError::Symbol {
            symbol: symbol.to_owned(),
            library,
        })
    }
}
