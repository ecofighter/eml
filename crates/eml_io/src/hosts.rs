//! 本物のHostを作る関数。`eml run`とREPLが使う。標準出力へ書き、子を作る間だけSIGINTとSIGTERMの塞ぎを外す。

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use eml_rt::{HostRef, StdHost};

use crate::mask::Unmasked;

/// 標準出力と標準エラーに書くHost。引数と標準入力を持たないので、REPLが使う。
pub fn stdio_host(file_root: PathBuf) -> HostRef {
    HostRef::new(Arc::new(
        StdHost::stdio(file_root).with_around_spawn(unmasked),
    ))
}

/// `eml run`のHost。標準出力と標準エラーに書き、スクリプトに`args`を渡し、標準入力を読ませる。
pub fn script_host(file_root: PathBuf, args: Vec<OsString>) -> HostRef {
    let host = StdHost::stdio(file_root)
        .with_args(args)
        .with_stdin(Box::new(io::stdin()))
        .with_around_spawn(unmasked);
    HostRef::new(Arc::new(host))
}

/// 子を1つ作る間だけ、呼んだスレッドでSIGINTとSIGTERMの塞ぎを外す。子は作ったスレッドの塞ぎを受け継ぐので、外さないと
/// worker 0のほかのworkerが始めた子をCtrl-CとSIGTERMで終わらせられない (docs/spec/stdlib.mdの「`Proc`」)。
fn unmasked(body: &mut dyn FnMut()) {
    let _unmasked = Unmasked::new();
    body();
}
