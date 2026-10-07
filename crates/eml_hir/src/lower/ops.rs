//! 演算子の列を fixity の表で木に組み直す (docs/spec/expressions.md の「演算子の列と単項マイナス」)。

use eml_diagnostics::{Diagnostic, Label, TextRange, TextSize};
use eml_syntax::ast::{self, OpSeqElement};

use super::expr::BodyLowering;
use super::{NameKind, NameUse, ambiguous, unresolved};
use crate::codes;
use crate::def_map::{NameRef, Resolved, ValueItem};
use crate::hir::{ExprId, ExprKind, Res};
use crate::item_tree::{Assoc, Fixity};

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
pub(super) const NEGATE_PRECEDENCE: u8 = 6;

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
        let mut undecided = false;
        for piece in &pieces {
            if let Piece::Operator { text, range } = piece {
                undecided |= self.undecided_operator(text, *range);
            }
        }
        if undecided {
            return self.alloc(ExprKind::Missing, range);
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
            // fixity は名前が解決した先の定義に付く (docs/spec/declarations.md の「fixity」)
            let Fixity { precedence, assoc } = self.fixity(&text);
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
                let callee = self.alloc(ExprKind::Path(Res::Function(self.lang.negate)), range);
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

    pub(super) fn binary(
        &mut self,
        op: &str,
        op_range: TextRange,
        lhs: ExprId,
        rhs: ExprId,
    ) -> ExprId {
        let range = self.exprs[lhs].range.cover(self.exprs[rhs].range);
        // 解決した先が Prelude の `&&` か `||` のときだけ、短絡評価にするため `if` に脱糖する。
        // ユーザーの定義は Prelude の演算子を隠すので、`&&` を定義すれば普通の呼び出しになる
        // (docs/spec/declarations.md の「fixity」)
        let res = match self.items.value(NameRef::Plain(op)) {
            Resolved::Found(ValueItem::Function(id)) if id == self.lang.and => {
                let otherwise = self.alloc(
                    ExprKind::Path(Res::Constructor(self.lang.false_ctor)),
                    op_range,
                );
                return self.alloc(
                    ExprKind::If {
                        condition: lhs,
                        then_branch: rhs,
                        else_branch: Some(otherwise),
                    },
                    range,
                );
            }
            Resolved::Found(ValueItem::Function(id)) if id == self.lang.or => {
                let then = self.alloc(
                    ExprKind::Path(Res::Constructor(self.lang.true_ctor)),
                    op_range,
                );
                return self.alloc(
                    ExprKind::If {
                        condition: lhs,
                        then_branch: then,
                        else_branch: Some(rhs),
                    },
                    range,
                );
            }
            Resolved::Found(ValueItem::Function(id)) => Some(Res::Function(id)),
            Resolved::Found(ValueItem::Constructor(ctor)) => Some(Res::Constructor(ctor)),
            Resolved::Found(ValueItem::Operation(operation)) => Some(Res::Operation(operation)),
            Resolved::NotFound if op == "::" => {
                let callee = self.unsupported(op_range, "lists are not supported yet");
                return self.alloc(
                    ExprKind::Call {
                        callee,
                        args: vec![lhs, rhs],
                    },
                    range,
                );
            }
            other => {
                if let Some(diagnostic) = unresolved(
                    &self.items,
                    self.file,
                    NameKind::Operator,
                    &NameUse::plain(op, op_range),
                    other,
                ) {
                    self.diagnostics.push(diagnostic);
                }
                None
            }
        };
        let callee = match res {
            Some(res) => self.alloc(ExprKind::Path(res), op_range),
            None => self.alloc(ExprKind::Missing, op_range),
        };
        self.alloc(
            ExprKind::Call {
                callee,
                args: vec![lhs, rhs],
            },
            range,
        )
    }

    /// 曖昧な演算子と壊れた import から来た演算子は、fixity が決まらない。既定の `infixl 9` で組むと E1006 や E1023 が
    /// 連鎖しうるので、そうした演算子を含む演算子の列、セクション、中置のパターンは、組まずに誤りにする。曖昧な演算子は
    /// ここで E1028 を出し、壊れた import の演算子は import で報告済みなので何も出さない
    /// (docs/implementation/architecture.md の「名前解決の回復」)。
    pub(super) fn undecided_operator(&mut self, op: &str, op_range: TextRange) -> bool {
        if self.items.fixity(NameRef::Plain(op)).is_some() {
            return false;
        }
        if let Resolved::Ambiguous(imports) = self.items.value(NameRef::Plain(op)) {
            let at = NameUse::plain(op, op_range);
            self.diagnostics.push(ambiguous(self.file, &at, &imports));
        }
        true
    }

    /// 組み直しに使う fixity。fixity の決まらない演算子は、呼ぶ側が先に `undecided_operator` で除く。
    pub(super) fn fixity(&self, op: &str) -> Fixity {
        self.items
            .fixity(NameRef::Plain(op))
            .unwrap_or(Fixity::DEFAULT)
    }
}
