//! オブジェクトのモデル、ヒープ、参照カウント、debug_heap の検査。
//!
//! 最初の段階では、実行結果の出力先 `OutputSink` だけを持つ。ヒープと参照カウントは後の段階で実装する。

mod output;

pub use output::OutputSink;
