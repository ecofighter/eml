//! プールの仕事の closure (docs/spec/vm.md の「操作の記録」)。仕事はプールのスレッドで動くので、ヒープにも RC にも
//! 触れず、所有して受け取った入力だけを読む。`Op::Job` を作るのはこのファイルだけで、`std` と `eml_rt::Op` だけを
//! 使い、`unsafe` を持たない。

use std::thread;
use std::time::Duration;

use eml_rt::Op;

/// `Std.IoTest.checksum` の仕事。`delay` だけ眠ってから、写した入力のバイトの和を返す。遅れは本物の時計でだけ
/// 正で、仮想の時計では 0 である (docs/spec/stdlib.md の「`Std.IoTest`」)。
pub(crate) fn checksum(input: Vec<u8>, delay: Duration) -> Op {
    Op::Job(Box::new(move || {
        thread::sleep(delay);
        Ok(input.iter().map(|&b| usize::from(b)).sum())
    }))
}
