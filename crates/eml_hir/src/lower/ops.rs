//! 演算子の列を fixity の表で木に組み直す (docs/spec/expressions.md の「演算子の列と単項マイナス」)。

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange, TextSize};
use eml_extern::FloatBits;
use eml_syntax::ast::{self, OpSeqElement};

use super::expr::BodyLowering;
use super::{NameKind, NameUse, ambiguous, unresolved};
use crate::codes;
use crate::def_map::{NameRef, Resolved};
use crate::hir::{ExprId, ExprKind, Literal, Res, ValueItem};
use crate::item_tree::Fixity;

enum Piece {
    Operand(ExprId, bool),
    Operator(Operator),
}

/// 列の中の演算子。名前は組み直す前に1回だけ引き、組み直しの fixity と `binary` の呼ぶ先が同じ結果を使う。
struct Operator {
    text: String,
    range: TextRange,
    resolved: Resolved<ValueItem>,
    fixity: Fixity,
}

struct Cursor {
    pieces: Vec<Piece>,
    pos: usize,
    /// 列の終わり。欠けた被演算子の位置に使う。
    end: TextSize,
}

impl Cursor {
    /// `at` の部品が演算子なら、その演算子。
    fn operator(&self, at: usize) -> Option<&Operator> {
        match self.pieces.get(at) {
            Some(Piece::Operator(operator)) => Some(operator),
            Some(Piece::Operand(..)) | None => None,
        }
    }
}

/// 同じ優先順位で結合しない2つの演算子の E1006。式とパターンの組み直しが同じ文言にする。
pub(super) fn non_associative(
    file: FileId,
    previous: &str,
    text: &str,
    range: TextRange,
) -> Diagnostic {
    Diagnostic::error(
        codes::NON_ASSOCIATIVE_OPERATORS,
        format!("`{previous}` and `{text}` cannot be combined without parentheses"),
        Label::new(file, range, "use parentheses to group the operators"),
    )
}

/// 単項の `-` は Haskell と同じく、優先順位 6 の `negate` として組み直す。
pub(super) const NEGATE_PRECEDENCE: u8 = 6;

impl BodyLowering<'_> {
    pub(super) fn lower_op_seq(&mut self, seq: &ast::OpSeq) -> ExprId {
        let range = seq.range();
        let mut pieces = Vec::new();
        let mut undecided = Vec::new();
        for element in seq.elements() {
            match element {
                OpSeqElement::Operand(expr) => {
                    let expr_range = expr.range();
                    let written_literal = matches!(expr, ast::Expr::Literal(_));
                    let id = self.lower_expr(Some(expr), expr_range);
                    let direct = written_literal
                        && matches!(
                            self.exprs[id].kind,
                            ExprKind::Literal(
                                Literal::Int(_) | Literal::Float(_) | Literal::Byte(_)
                            )
                        );
                    pieces.push(Piece::Operand(id, direct));
                }
                OpSeqElement::Operator(token) => {
                    let text = token.text().to_string();
                    let op_range = token.text_range();
                    let resolved = self.items.value(NameRef::Plain(&text));
                    match self.items.fixity_of(&resolved) {
                        Some(fixity) => pieces.push(Piece::Operator(Operator {
                            text,
                            range: op_range,
                            resolved,
                            fixity,
                        })),
                        None => undecided.push((text, op_range, resolved)),
                    }
                }
            }
        }
        if !undecided.is_empty() {
            for (text, op_range, resolved) in &undecided {
                self.report_undecided(text, *op_range, resolved);
            }
            return self.alloc(ExprKind::Missing, range);
        }
        let mut cursor = Cursor {
            pieces,
            pos: 0,
            end: range.end(),
        };
        self.climb(&mut cursor, 0, None)
    }

    /// `previous` は直前に組んだ演算子、または囲む演算子の部品の位置である。同じ優先順位で結合しない並びを見つけるために
    /// 使う。
    fn climb(
        &mut self,
        cursor: &mut Cursor,
        min_precedence: u8,
        mut previous: Option<usize>,
    ) -> ExprId {
        let mut lhs = self.operand(cursor, min_precedence);
        // fixity は名前が解決した先の定義に付く (docs/spec/declarations.md の「fixity」)
        while let Some(fixity) = cursor.operator(cursor.pos).map(|operator| operator.fixity) {
            if fixity.precedence < min_precedence {
                break;
            }
            let at = cursor.pos;
            cursor.pos += 1;
            let rhs = self.climb(cursor, fixity.right_min(), Some(at));
            let operator = cursor.operator(at).expect("`at` is an operator");
            match previous.and_then(|previous| cursor.operator(previous)) {
                Some(before) if before.fixity.conflicts_with(fixity) => {
                    self.diagnostics.push(non_associative(
                        self.file,
                        &before.text,
                        &operator.text,
                        operator.range,
                    ));
                    // 組み方が決まらないので、型の誤りを連鎖させないように式全体を Missing にする
                    let whole = self.exprs[lhs].range.cover(self.exprs[rhs].range);
                    lhs = self.alloc(ExprKind::Missing, whole);
                }
                _ => {
                    lhs = self.binary(
                        &operator.text,
                        operator.range,
                        operator.resolved.clone(),
                        lhs,
                        rhs,
                    );
                }
            }
            previous = Some(at);
        }
        lhs
    }

    fn operand(&mut self, cursor: &mut Cursor, min_precedence: u8) -> ExprId {
        match cursor.pieces.get(cursor.pos) {
            Some(Piece::Operator(operator)) if operator.text == "-" => {
                let range = operator.range;
                cursor.pos += 1;
                if min_precedence > NEGATE_PRECEDENCE {
                    self.diagnostics.push(Diagnostic::error(
                        codes::NON_ASSOCIATIVE_OPERATORS,
                        "a prefix `-` cannot appear here without parentheses",
                        Label::new(self.file, range, "put the negation in parentheses"),
                    ));
                }
                let literal_at = cursor.pos;
                let operand = self.climb(cursor, NEGATE_PRECEDENCE + 1, None);
                // 負の数のリテラルとして1つにまとめる。`negate` の呼び出しにしないのは、`Panic` を起こさない
                // 定数にするため (docs/spec/expressions.md の「演算子の列と単項マイナス」)。
                if cursor.pos == literal_at + 1
                    && matches!(cursor.pieces[literal_at], Piece::Operand(_, true))
                {
                    let negated = match self.exprs[operand].kind {
                        ExprKind::Literal(Literal::Int(n)) => Some(Literal::Int(-n)),
                        ExprKind::Literal(Literal::Float(x)) => {
                            Some(Literal::Float(FloatBits::new(-x.value())))
                        }
                        _ => None,
                    };
                    if let Some(literal) = negated {
                        return self.alloc(
                            ExprKind::Literal(literal),
                            range.cover(self.exprs[operand].range),
                        );
                    }
                    if matches!(
                        self.exprs[operand].kind,
                        ExprKind::Literal(Literal::Byte(_))
                    ) {
                        let whole = range.cover(self.exprs[operand].range);
                        self.negative_byte_literal(whole);
                        return self.alloc(ExprKind::Missing, whole);
                    }
                }
                let callee = self.alloc(
                    ExprKind::Path(Res::Item(ValueItem::Method(self.lang.negate))),
                    range,
                );
                let whole = range.cover(self.exprs[operand].range);
                self.alloc(
                    ExprKind::Call {
                        callee,
                        args: vec![operand],
                    },
                    whole,
                )
            }
            Some(&Piece::Operand(expr, _)) => {
                cursor.pos += 1;
                expr
            }
            // 欠けた被演算子はパーサが報告済み
            Some(Piece::Operator(operator)) => self.alloc(ExprKind::Missing, operator.range),
            None => self.alloc(ExprKind::Missing, TextRange::empty(cursor.end)),
        }
    }

    /// `resolved` は `op` を値の名前として引いた結果である。呼ぶ側が fixity を求めるのに引いた結果を渡し、
    /// 同じトークンを引き直さない。
    pub(super) fn binary(
        &mut self,
        op: &str,
        op_range: TextRange,
        resolved: Resolved<ValueItem>,
        lhs: ExprId,
        rhs: ExprId,
    ) -> ExprId {
        let range = self.exprs[lhs].range.cover(self.exprs[rhs].range);
        // 解決した先が Prelude の `&&` か `||` のときだけ、短絡評価にするため `if` に脱糖する。
        // ユーザーの定義は Prelude の演算子を隠すので、`&&` を定義すれば普通の呼び出しになる
        // (docs/spec/declarations.md の「fixity」)
        let res = match resolved {
            Resolved::Found(ValueItem::Function(id)) if id == self.lang.and => {
                let otherwise = self.alloc(
                    ExprKind::Path(Res::Item(ValueItem::Constructor(self.lang.false_ctor))),
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
                    ExprKind::Path(Res::Item(ValueItem::Constructor(self.lang.true_ctor))),
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
            Resolved::Found(item) => Some(Res::Item(item)),
            other => {
                self.diagnostics.extend(unresolved(
                    &self.items,
                    self.file,
                    NameKind::Operator,
                    &NameUse::plain(op, op_range),
                    other,
                ));
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

    /// 曖昧な演算子と壊れた import から来た演算子は、fixity が決まらない (`Resolver::fixity_of` が `None`)。既定の
    /// `infixl 9` で組むと E1006 や E1023 が連鎖しうるので、そうした演算子を含む演算子の列、セクション、中置のパターンは、
    /// 組まずに誤りにする。曖昧な演算子はここで E1028 を出し、壊れた import の演算子は import で報告済みなので何も
    /// 出さない (docs/implementation/architecture.md の「名前解決の回復」)。
    pub(super) fn report_undecided(
        &mut self,
        op: &str,
        op_range: TextRange,
        resolved: &Resolved<ValueItem>,
    ) {
        if let Resolved::Ambiguous(imports) = resolved {
            let at = NameUse::plain(op, op_range);
            self.diagnostics.push(ambiguous(self.file, &at, imports));
        }
    }
}
