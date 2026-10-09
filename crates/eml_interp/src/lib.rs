use std::path::PathBuf;

use eml_core_ir::Program;
use eml_runtime::OutputSink;

mod effects;
mod error;
mod externs;
mod machine;
mod runtime;

pub use error::{Fault, RuntimeError, SourceLocation};

use machine::Machine;
use runtime::Runtime;

/// フィールドを足しても呼び出し側を壊さないように、`non_exhaustive` にして `RunConfig::default()` から作らせる。
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct RunConfig {
    pub debug_heap: bool,
    /// `open` の相対パスの基準 (docs/spec/effects.md の「組み込みの `IO`」)。既定の空のパスはカレントディレクトリを指す。
    pub file_root: PathBuf,
}

impl RunConfig {
    pub fn with_debug_heap(mut self, debug_heap: bool) -> Self {
        self.debug_heap = debug_heap;
        self
    }

    pub fn with_file_root(mut self, root: PathBuf) -> Self {
        self.file_root = root;
        self
    }
}

/// 実行の仕事の回数と、同時に生きていたヒープの物体の数の最大。時間ではなく回数を比べて、実行の費用が入力の大きさに
/// 比例して伸びることをテストで確かめるために数える (docs/spec/runtime.md の「実行の API」)。数えるものを後で足しても
/// 呼び出し側を壊さないよう、`non_exhaustive` にする。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunStats {
    /// `perform` が handler を探すときに調べたフレームの数。
    pub handler_visits: u64,
    /// 文字列の物体の中身に書いたバイト数。`String` の容量の再確保と、出力への書き込みは数えない。
    pub string_bytes_copied: u64,
    /// ヒープの物体の参照の数を増やした回数。`dup` と、不死の物体の参照を作る回数を数える。
    pub rc_increments: u64,
    /// ヒープの物体の参照の数を減らした回数。解放の連鎖で子やフレームの数を減らした分も数える。物体の解放
    /// そのものは数えない。
    pub rc_decrements: u64,
    /// 実行した `box` の文の数。インタプリタでは値をそのまま渡すので費用はほとんどないが、S9 の語の値の表現では
    /// 費用になる変換なので、今のうちから数を追う。
    pub boxes: u64,
    /// 実行した `unbox` の文の数。数える理由は `boxes` と同じである。
    pub unboxes: u64,
    /// 同時に生きていたヒープの物体の数の最大。フレームと不死のリテラルを含む。仕事の回数ではないので、何もしない
    /// プログラムでも `Frame::Root` のフレームの分だけ 0 にならない。フレームの伸びはこの数でしか見えず、末尾呼び出しを
    /// 失うと反復の数に比例して増える。
    pub peak_objects: u64,
}

/// 実行時エラーとリークのときは `RunStats` を返さない。回数を比べるテストは、正常に終わった実行だけを見る。
pub fn run(
    program: &Program,
    config: &RunConfig,
    out: &OutputSink,
) -> Result<RunStats, RuntimeError> {
    let mut machine = Machine::new(program, out, &config.file_root);
    machine.run()?;
    finish(&machine.rt, config)
}

/// 正常に終わった実行の後始末。実行時エラーで止まった場合はリークを数えない。途中のフレームが残っているのは当然だから。
fn finish(rt: &Runtime<'_>, config: &RunConfig) -> Result<RunStats, RuntimeError> {
    if config.debug_heap {
        rt.check_leaks()?;
    }
    Ok(rt.stats())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_debug_heap_sets_the_flag() {
        assert!(RunConfig::default().with_debug_heap(true).debug_heap);
        assert!(!RunConfig::default().with_debug_heap(false).debug_heap);
    }
}
