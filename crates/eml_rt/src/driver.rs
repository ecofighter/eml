//! worker ごとの時計と待ち (docs/spec/vm.md の「時計と driver」)。実行系が持ち、`Ctx` には入れない。
//!
//! 時間だけを扱う trait なので、`Host` の trait と同じく `verus!` の外に置く。`external_body` を持たないため、
//! 信頼の一覧に行は要らない。

use std::time::Duration;

/// worker ごとの時計と待ち。持ち主の worker だけが触るので、`&mut self` で呼び、`Sync` は求めない。
pub trait IoDriver: Send {
    /// 作ったときからの単調な経過時間。仮想の時計はミリ秒の整数で進む。
    fn now(&self) -> Duration;
    /// `now()` が `deadline_ms` ミリ秒以上になるまで待つ。早く戻ってよいので、呼ぶ側が時刻を確かめ直す。本物の時計は
    /// スレッドを止め、仮想の時計は `now` を `deadline_ms` に進める。
    fn wait_until(&mut self, deadline_ms: u64);
}
