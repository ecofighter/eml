//! `eml_rt`の物体の配置を読む唯一のmodule (docs/spec/object-model.md の「語の位置」)。メモリに触るコード生成をここに置き、
//! 生成したコードが頼る配置の事実を1か所で`eml_rt`に結び付ける。
use eml_lir::Shape;
use eml_rt::layout;

// reuseは走査する語が`REUSE_SCANNED_LIMIT`個未満の形だけを組にし、ネイティブのコードはそれを数の語なしで`eml_rt`の上に
// 下ろすので、上限は`eml_rt`の上限以下でなければならない (docs/spec/object-model.md の「ペイロード」)
const _: () = assert!(Shape::REUSE_SCANNED_LIMIT as usize <= layout::INLINE_SCAN_LIMIT);
