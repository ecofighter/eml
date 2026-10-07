//! 演算子の参照とセクションを、ラムダに脱糖する (docs/spec/expressions.md の「セクション」)。本体の二項演算は、演算子の
//! 列の組み直しと同じ `binary` で作るので、`&&` の短絡、中置のコンストラクタ、ユーザーの演算子がそのまま効く。`==` の
//! 比べ方は、ラムダの本体の呼び出しについて型検査が決める。

use eml_diagnostics::{Diagnostic, Label, TextRange};
use eml_syntax::SyntaxToken;
use eml_syntax::ast::{self, OpSeqElement};

use super::expr::BodyLowering;
use super::ops::NEGATE_PRECEDENCE;
use crate::codes;
use crate::hir::*;
use crate::item_tree::{Assoc, Fixity};

/// 空いている被演算子の側。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hole {
    Left,
    Right,
}

impl BodyLowering<'_> {
    pub(super) fn lower_op_ref(&mut self, op_ref: &ast::OpRef, range: TextRange) -> ExprId {
        let Some(op) = op_ref.operator() else {
            return self.alloc(ExprKind::Missing, range);
        };
        let (a_pat, a) = self.hidden_param("$a", range);
        let (b_pat, b) = self.hidden_param("$b", range);
        let body = self.binary(op.text(), op.text_range(), a, b);
        self.alloc(
            ExprKind::Lambda(Closure {
                params: vec![a_pat, b_pat],
                body,
            }),
            range,
        )
    }

    pub(super) fn lower_left_section(
        &mut self,
        section: &ast::LeftSection,
        range: TextRange,
    ) -> ExprId {
        match section.operator() {
            Some(op) => self.section(&op, section.operand(), Hole::Right, range),
            None => self.alloc(ExprKind::Missing, range),
        }
    }

    pub(super) fn lower_right_section(
        &mut self,
        section: &ast::RightSection,
        range: TextRange,
    ) -> ExprId {
        match section.operator() {
            Some(op) => self.section(&op, section.operand(), Hole::Left, range),
            None => self.alloc(ExprKind::Missing, range),
        }
    }

    /// 被演算子は、spec のとおりラムダを呼ぶたびに評価する。
    fn section(
        &mut self,
        op: &SyntaxToken,
        operand: Option<ast::Expr>,
        hole: Hole,
        range: TextRange,
    ) -> ExprId {
        if let Some(ast::Expr::OpSeq(seq)) = &operand
            && let Some(inner) = self.looser_operator(op.text(), seq, hole)
        {
            self.lower_discarded(operand);
            self.diagnostics.push(
                Diagnostic::error(
                    codes::INVALID_SECTION,
                    format!(
                        "the section of `{}` needs parentheses around its operand",
                        op.text()
                    ),
                    Label::new(
                        self.file,
                        range,
                        format!(
                            "`{}` does not bind more tightly than `{}`",
                            inner.text(),
                            op.text()
                        ),
                    ),
                )
                .with_help("put the operand in parentheses"),
            );
            // 組み方が決まらないので、型の誤りを連鎖させないようにセクション全体を Missing にする
            return self.alloc(ExprKind::Missing, range);
        }
        let leading_minus = match &operand {
            Some(ast::Expr::OpSeq(seq)) if hole == Hole::Left => match seq.elements().next() {
                Some(OpSeqElement::Operator(token)) if token.text() == "-" => Some(token),
                _ => None,
            },
            _ => None,
        };
        if let Some(minus) = leading_minus
            && self.right_operand_minimum(op.text()) > NEGATE_PRECEDENCE
        {
            self.diagnostics.push(Diagnostic::error(
                codes::NON_ASSOCIATIVE_OPERATORS,
                "a prefix `-` cannot appear here without parentheses",
                Label::new(
                    self.file,
                    minus.text_range(),
                    "put the negation in parentheses",
                ),
            ));
            self.lower_discarded(operand);
            return self.alloc(ExprKind::Missing, range);
        }
        let operand_range = operand.as_ref().map_or(range, |operand| operand.range());
        let value = self.lower_expr(operand, operand_range);
        let (pat, x) = self.hidden_param("$x", range);
        let body = match hole {
            Hole::Left => self.binary(op.text(), op.text_range(), x, value),
            Hole::Right => self.binary(op.text(), op.text_range(), value, x),
        };
        self.alloc(
            ExprKind::Lambda(Closure {
                params: vec![pat],
                body,
            }),
            range,
        )
    }

    /// 診断だけが目的の被演算子の lowering。名前の誤りなどを、無効なセクションの中でも報告するために使う。
    fn lower_discarded(&mut self, operand: Option<ast::Expr>) {
        if let Some(operand) = operand {
            let range = operand.range();
            self.lower_expr(Some(operand), range);
        }
    }

    /// `$x op e` と組むときの、右の被演算子に許される最小の優先順位。`climb` が演算子の直後で使う値と同じ。
    fn right_operand_minimum(&self, op: &str) -> u8 {
        let Fixity { precedence, assoc } = self.fixity(op);
        if assoc == Assoc::Right {
            precedence
        } else {
            precedence + 1
        }
    }

    /// セクションの演算子を根にして組めない、被演算子の中の二項演算子。被演算子の中の演算子は、セクションの演算子より
    /// 強く結合するか、優先順位が同じで両方の結合の向きが空いた側に合っていなければならない (docs/spec/expressions.md の
    /// 「セクション」)。演算子の直後の `-` は前置の `-` で、被演算子自身の組み直しが扱う。
    /// 右が空いたセクションの先頭の `-` は、優先順位 6 の左結合の演算子として数える (`(- 2 *)` は E1023、`(- 2 +)` は可)。
    /// 左が空いたセクションの先頭の `-` は、`section` が E1006 として検査する。
    fn looser_operator(&self, op: &str, seq: &ast::OpSeq, hole: Hole) -> Option<SyntaxToken> {
        let outer = self.fixity(op);
        // 左が空いていれば `$x op (e)` と組むので右結合、右が空いていれば `(e) op $x` と組むので左結合が合う
        let toward_hole = match hole {
            Hole::Left => Assoc::Right,
            Hole::Right => Assoc::Left,
        };
        let mut after_operator = true;
        let mut first = true;
        for element in seq.elements() {
            let is_first = std::mem::take(&mut first);
            match element {
                OpSeqElement::Operand(_) => after_operator = false,
                OpSeqElement::Operator(token) => {
                    let prefix = after_operator;
                    after_operator = true;
                    let inner = if prefix && is_first && hole == Hole::Right && token.text() == "-"
                    {
                        Fixity {
                            precedence: NEGATE_PRECEDENCE,
                            assoc: Assoc::Left,
                        }
                    } else if prefix {
                        continue;
                    } else {
                        self.fixity(token.text())
                    };
                    let tighter = inner.precedence > outer.precedence;
                    let same_side = inner.precedence == outer.precedence
                        && inner.assoc == toward_hole
                        && outer.assoc == toward_hole;
                    if !tighter && !same_side {
                        return Some(token);
                    }
                }
            }
        }
        None
    }
}
