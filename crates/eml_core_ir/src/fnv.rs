//! 名前のハッシュに使う 64 ビットの FNV-1a。translate の instance の名前と、optimize の複製の名前が使う
//! (docs/spec/core-ir.md の「変換の規則」)。

/// 名前はプロセスをまたいで同じでなければならないので、種を乱数で選ぶ `std` のハッシュは使わない。
pub(crate) struct Fnv(pub(crate) u64);

impl Fnv {
    pub(crate) fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    pub(crate) fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    pub(crate) fn number(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    /// 長さを先に混ぜ、続く値と境目がずれないようにする。
    pub(crate) fn text(&mut self, text: &str) {
        self.number(text.len() as u64);
        self.bytes(text.as_bytes());
    }
}
