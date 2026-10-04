//! 演算子の列を fixity の表で木に組み直す (docs/spec/expressions.md の「演算子の列と単項マイナス」)。

use eml_diagnostics::{Diagnostic, Label, TextRange, TextSize};
use eml_syntax::ast::{self, OpSeqElement};

use super::expr::BodyLowering;
use crate::builtin::{Assoc, Builtin, fixity};
use crate::codes;
use crate::hir::{ExprId, ExprKind, Res};

#[derive(Clone)]
enum Piece {
    Operand(ExprId),
    Operator { text: String, range: TextRange },
}

struct Cursor {
    pieces: Vec<Piece>,
    pos: usize,
    /// 列の終わり。欠けた被演算子の位置に使う。
    end: TextSize,
}

impl Cursor {
    fn peek(&self) -> Option<Piece> {
        self.pieces.get(self.pos).cloned()
    }
}

/// 単項の `-` は Haskell と同じく、優先順位 6 の `negate` として組み直す。
const NEGATE_PRECEDENCE: u8 = 6;

impl BodyLowering<'_> {
    pub(super) fn lower_op_seq(&mut self, seq: &ast::OpSeq) -> ExprId {
        let range = seq.range();
        let mut pieces = Vec::new();
        for element in seq.elements() {
            match element {
                OpSeqElement::Operand(expr) => {
                    let expr_range = expr.range();
                    pieces.push(Piece::Operand(self.lower_expr(Some(expr), expr_range)));
                }
                OpSeqElement::Operator(token) => pieces.push(Piece::Operator {
                    text: token.text().to_string(),
                    range: token.text_range(),
                }),
            }
        }
        let mut cursor = Cursor {
            pieces,
            pos: 0,
            end: range.end(),
        };
        self.climb(&mut cursor, 0, None)
    }

    fn climb(
        &mut self,
        cursor: &mut Cursor,
        min_precedence: u8,
        mut previous: Option<(String, u8, Assoc)>,
    ) -> ExprId {
        let mut lhs = self.operand(cursor, min_precedence);
        while let Some(Piece::Operator { text, range }) = cursor.peek() {
            // fixity の宣言がない演算子は `infixl 9` とする (docs/spec/declarations.md)
            let (precedence, assoc) = fixity(&text).unwrap_or((9, Assoc::Left));
            if precedence < min_precedence {
                break;
            }
            cursor.pos += 1;
            // `previous` は直前に組んだ演算子、または囲む演算子。同じ優先順位で結合しない並びを見つけるために使う
            let conflict = previous
                .as_ref()
                .is_some_and(|(_, p, a)| *p == precedence && (*a != assoc || assoc == Assoc::None));
            let next_min = if assoc == Assoc::Right {
                precedence
            } else {
                precedence + 1
            };
            let rhs = self.climb(cursor, next_min, Some((text.clone(), precedence, assoc)));
            if conflict {
                let (previous_text, _, _) = previous.as_ref().unwrap();
                self.diagnostics.push(Diagnostic::error(
                    codes::NON_ASSOCIATIVE_OPERATORS,
                    format!(
                        "`{previous_text}` and `{text}` cannot be combined without parentheses"
                    ),
                    Label::new(self.file, range, "use parentheses to group the operators"),
                ));
                // 組み方が決まらないので、型の誤りを連鎖させないように式全体を Missing にする
                let whole = self.exprs[lhs].range.cover(self.exprs[rhs].range);
                lhs = self.alloc(ExprKind::Missing, whole);
            } else {
                lhs = self.binary(&text, range, lhs, rhs);
            }
            previous = Some((text, precedence, assoc));
        }
        lhs
    }

    fn operand(&mut self, cursor: &mut Cursor, min_precedence: u8) -> ExprId {
        match cursor.peek() {
            Some(Piece::Operator { text, range }) if text == "-" => {
                cursor.pos += 1;
                if min_precedence > NEGATE_PRECEDENCE {
                    self.diagnostics.push(Diagnostic::error(
                        codes::NON_ASSOCIATIVE_OPERATORS,
                        "a prefix `-` cannot appear here without parentheses",
                        Label::new(self.file, range, "put the negation in parentheses"),
                    ));
                }
                let operand = self.climb(cursor, NEGATE_PRECEDENCE + 1, None);
                let callee = self.alloc(ExprKind::Path(Res::Builtin(Builtin::IntNeg)), range);
                let whole = range.cover(self.exprs[operand].range);
                self.alloc(
                    ExprKind::Call {
                        callee,
                        args: vec![operand],
                    },
                    whole,
                )
            }
            Some(Piece::Operand(expr)) => {
                cursor.pos += 1;
                expr
            }
            // 欠けた被演算子はパーサが報告済み
            Some(Piece::Operator { range, .. }) => self.alloc(ExprKind::Missing, range),
            None => self.alloc(ExprKind::Missing, TextRange::empty(cursor.end)),
        }
    }

    fn binary(&mut self, op: &str, op_range: TextRange, lhs: ExprId, rhs: ExprId) -> ExprId {
        let range = self.exprs[lhs].range.cover(self.exprs[rhs].range);
        match op {
            // 短絡評価にするため `if` に脱糖する (docs/spec/declarations.md)
            "&&" => {
                let otherwise = self.alloc(ExprKind::Path(Res::Builtin(Builtin::False)), op_range);
                self.alloc(
                    ExprKind::If {
                        condition: lhs,
                        then_branch: rhs,
                        else_branch: Some(otherwise),
                    },
                    range,
                )
            }
            "||" => {
                let then = self.alloc(ExprKind::Path(Res::Builtin(Builtin::True)), op_range);
                self.alloc(
                    ExprKind::If {
                        condition: lhs,
                        then_branch: then,
                        else_branch: Some(rhs),
                    },
                    range,
                )
            }
            "|>" => self.pipe(lhs, rhs, range),
            "<|" => self.call(lhs, vec![rhs], range),
            _ => {
                let callee = match Builtin::binary_operator(op) {
                    Some(builtin) => self.alloc(ExprKind::Path(Res::Builtin(builtin)), op_range),
                    None if op == "::" => self.unsupported(op_range, "lists are not supported yet"),
                    None => {
                        self.diagnostics.push(Diagnostic::error(
                            codes::UNDEFINED_NAME,
                            format!("cannot find operator `{op}`"),
                            Label::new(self.file, op_range, "not found in this scope"),
                        ));
                        self.alloc(ExprKind::Missing, op_range)
                    }
                };
                self.alloc(
                    ExprKind::Call {
                        callee,
                        args: vec![lhs, rhs],
                    },
                    range,
                )
            }
        }
    }
}
