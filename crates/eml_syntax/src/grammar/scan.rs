//! 括弧とブロックの深さを数え、範囲の終わりを決める。読み飛ばしと先読みが同じ規則で終わりを決めるため。

use crate::SyntaxKind::{self, *};

/// 括弧の深さとブロックの深さを別々に数える。規則 2 でレイアウト段が括弧を暗黙に閉じると、閉じ括弧のトークンが
/// ないまま括弧の深さが戻らない。そのため、ブロックの外の `SEP` と `CLOSE` は、括弧の深さにかかわらず範囲の
/// 終わりとする。ファイルの残りを飲み込まないための同期点になる (docs/spec/layout.md の規則 2)。
///
/// 補間の穴 (`INTERP_START` と `INTERP_END`) も入れ子の対として数える。穴の中の括弧の読み飛ばしを、穴の閉じで
/// 止めるため (docs/implementation/architecture.md の「構文解析の回復」)。
#[derive(Debug, Default)]
pub(super) struct Nesting {
    brackets: u32,
    blocks: u32,
    interps: u32,
}

impl Nesting {
    /// `kind` が今の範囲を終わらせるか。対応する開き括弧のない閉じ括弧、ブロックの外の `CLOSE` と `SEP`、
    /// 対応する開きのない穴の閉じ、ファイルの終わりである。
    pub(super) fn ends(&self, kind: SyntaxKind) -> bool {
        match kind {
            EOF => true,
            INTERP_END => self.interps == 0,
            LAYOUT_SEP | LAYOUT_CLOSE => self.blocks == 0,
            kind if kind.is_closing_bracket() => self.brackets == 0,
            _ => false,
        }
    }

    /// `kind` を読んだ後の深さにする。対応のない閉じは深さを変えない。
    pub(super) fn step(&mut self, kind: SyntaxKind) {
        match kind {
            LAYOUT_OPEN => self.blocks += 1,
            LAYOUT_CLOSE => self.blocks = self.blocks.saturating_sub(1),
            INTERP_START => self.interps += 1,
            INTERP_END => self.interps = self.interps.saturating_sub(1),
            kind if kind.is_opening_bracket() => self.brackets += 1,
            kind if kind.is_closing_bracket() => self.brackets = self.brackets.saturating_sub(1),
            _ => {}
        }
    }

    pub(super) fn in_block(&self) -> bool {
        self.blocks > 0
    }

    pub(super) fn at_top(&self) -> bool {
        self.brackets == 0 && self.blocks == 0 && self.interps == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ends_after(kinds: &[SyntaxKind], last: SyntaxKind) -> bool {
        let mut nesting = Nesting::default();
        for &kind in kinds {
            assert!(!nesting.ends(kind), "{kind:?} ended the range early");
            nesting.step(kind);
        }
        nesting.ends(last)
    }

    #[test]
    fn an_unmatched_closing_bracket_ends_the_range() {
        assert!(ends_after(&[], R_PAREN));
        assert!(!ends_after(&[L_PAREN], R_PAREN));
        assert!(ends_after(&[L_PAREN, R_PAREN], R_BRACK));
    }

    #[test]
    fn a_separator_outside_blocks_ends_the_range_even_inside_brackets() {
        // 規則 2 でレイアウト段が括弧を暗黙に閉じると、閉じ括弧のトークンがないまま SEP が来る
        // (docs/spec/layout.md の規則 2)。
        assert!(ends_after(&[], LAYOUT_SEP));
        assert!(ends_after(&[L_PAREN], LAYOUT_SEP));
        assert!(!ends_after(&[LAYOUT_OPEN], LAYOUT_SEP));
        assert!(!ends_after(&[L_PAREN, LAYOUT_OPEN], LAYOUT_SEP));
    }

    #[test]
    fn an_unmatched_block_close_and_the_end_of_file_end_the_range() {
        assert!(ends_after(&[], LAYOUT_CLOSE));
        assert!(!ends_after(&[LAYOUT_OPEN], LAYOUT_CLOSE));
        assert!(ends_after(&[L_PAREN, LAYOUT_OPEN], EOF));
    }

    #[test]
    fn an_unmatched_interpolation_end_ends_the_range() {
        assert!(ends_after(&[], INTERP_END));
        assert!(!ends_after(&[INTERP_START], INTERP_END));
    }

    #[test]
    fn depth_is_tracked_separately_for_brackets_and_blocks() {
        let mut nesting = Nesting::default();
        assert!(nesting.at_top());
        nesting.step(L_PAREN);
        assert!(!nesting.at_top() && !nesting.in_block());
        nesting.step(LAYOUT_OPEN);
        assert!(nesting.in_block());
        nesting.step(LAYOUT_CLOSE);
        nesting.step(R_PAREN);
        assert!(nesting.at_top());
    }
}
