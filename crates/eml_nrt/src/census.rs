//! census。成功したrunの後にworkerのヒープに残った、数える物体を数える (docs/spec/native.md の「実行の始めと終わり」)。
//! VMの検査モードに代わる、ネイティブのコードの参照の数の安全網である。
#![allow(unsafe_code)]

use std::collections::BTreeMap;

use eml_rt::{Ctx, Fast};

/// 数える物体のうち残ったものの、種類の名前と数を、名前の順に返す。不死の物体 (ダミー、定数) は数えない。
pub(crate) fn census(ctx: &mut Ctx<Fast>) -> Vec<(String, u64)> {
    ctx.reclaim();
    // SAFETY: censusは`Ctx`の持つ語を変えず、物体を作らない
    let heap = unsafe { ctx.heap_mut() };
    let mut counts = BTreeMap::<&'static str, u64>::new();
    for w in heap.live_objects() {
        // SAFETY: `w`はこのヒープの`live_objects`から来たので、このヒープの生きている物体を指す
        if !unsafe { heap.is_immortal(w) } {
            // SAFETY: 上と同じ
            let kind = unsafe { heap.meta(w) }.map_or("?", |m| m.kind().name());
            *counts.entry(kind).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .map(|(kind, n)| (kind.to_string(), n))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use eml_rt::{Buffering, Control, Ctx, Fast, HostRef, Limits, StdHost};

    use super::census;

    fn ctx() -> Ctx<Fast> {
        let host = StdHost::new(
            Box::new(std::io::sink()),
            Box::new(std::io::sink()),
            std::path::PathBuf::new(),
            Buffering::Block,
        );
        let control = Arc::new(Control::new(Limits::default()));
        Ctx::new(control, HostRef::new(Arc::new(host)))
    }

    #[test]
    fn a_fresh_context_has_nothing_to_report() {
        assert_eq!(census(&mut ctx()), []);
    }

    #[test]
    fn counted_objects_are_reported_by_kind() {
        let mut ctx = ctx();
        // SAFETY: 新しい物体は、作る関数が中身まで書き終える
        let heap = unsafe { ctx.heap_mut() };
        heap.alloc_data(0, 0, 0, false);
        heap.alloc_data(1, 0, 1, false);
        heap.alloc_string("left");
        assert_eq!(
            census(&mut ctx),
            [("Data".to_string(), 2), ("String".to_string(), 1)]
        );
    }

    #[test]
    fn immortal_objects_are_not_counted() {
        let mut ctx = ctx();
        // SAFETY: 上と同じ
        let heap = unsafe { ctx.heap_mut() };
        heap.alloc_string_immortal("constant");
        let w = heap.alloc_data(0, 0, 0, false);
        // SAFETY: `w`は生きている物体へのただ1つの参照である
        unsafe { heap.decref(w) }.unwrap();
        assert_eq!(census(&mut ctx), []);
    }
}
