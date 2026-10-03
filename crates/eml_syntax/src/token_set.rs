use crate::SyntaxKind;

/// トークンの種類の集合。エラー回復の同期点などに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TokenSet(u128);

impl TokenSet {
    pub(crate) const fn new(kinds: &[SyntaxKind]) -> TokenSet {
        let mut bits = 0u128;
        let mut i = 0;
        while i < kinds.len() {
            bits |= mask(kinds[i]);
            i += 1;
        }
        TokenSet(bits)
    }

    pub(crate) const fn contains(&self, kind: SyntaxKind) -> bool {
        self.0 & mask(kind) != 0
    }
}

const fn mask(kind: SyntaxKind) -> u128 {
    assert!((kind as u16) < 128, "only token kinds can be in a TokenSet");
    1u128 << (kind as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_only_listed_kinds() {
        let set = TokenSet::new(&[SyntaxKind::FN_KW, SyntaxKind::EOF]);
        assert!(set.contains(SyntaxKind::FN_KW));
        assert!(set.contains(SyntaxKind::EOF));
        assert!(!set.contains(SyntaxKind::TYPE_KW));
    }
}
