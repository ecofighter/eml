//! 文字列の extern (docs/spec/object-model.md の「extern」)。比較、`++`、`Int.show`、`String.show` の結果を、バイト列の
//! spec (`lex`、`decimal`、`escape`) に対して証明する。文字列の物体を読み書きする部品はヒープの `string` にある。

use std::cmp::Ordering;

#[cfg(verus_only)]
use vstd::string::StringSliceAdditionalSpecFns;
#[cfg(verus_only)]
use vstd::utf8::{
    encode_scalar, encode_utf8, encode_utf8_valid_utf8, is_char_boundary,
    is_char_boundary_iff_not_is_continuation_byte, is_char_boundary_start_end_of_seq,
    pop_first_scalar, valid_first_scalar, valid_utf8, valid_utf8_concat, valid_utf8_split,
};

#[cfg(verus_only)]
use super::super::Core;
use super::super::Heap;
#[cfg(verus_only)]
use super::proof::*;
use super::*;
use crate::alloc::too_large;
use crate::externs::{
    ArithFault, EQ, FALSE, Fault, GT, LT, TRUE, float_fixed, float_parse, float_repr,
};

verus! {

/// バイト列の辞書式の順序。先頭から比べて最初に違うバイトの大小で決め、片方が尽きたら短い方が小さい。
pub open spec fn lex(a: Seq<u8>, b: Seq<u8>) -> i64
    decreases a.len(),
{
    if a.len() == 0 {
        if b.len() == 0 {
            EQ
        } else {
            LT
        }
    } else if b.len() == 0 {
        GT
    } else if a[0] < b[0] {
        LT
    } else if a[0] > b[0] {
        GT
    } else {
        lex(a.drop_first(), b.drop_first())
    }
}

/// 0 以上の整数の10進の数字。先頭に 0 を置かない。
pub open spec fn digits(n: nat) -> Seq<u8>
    decreases n,
{
    if n < 10 {
        seq![(b'0' + n) as u8]
    } else {
        digits(n / 10) + seq![(b'0' + n % 10) as u8]
    }
}

/// `Int.show` の結果。負なら `-` を前に付ける。
pub open spec fn decimal(i: int) -> Seq<u8> {
    if i < 0 {
        seq![b'-'] + digits((-i) as nat)
    } else {
        digits(i as nat)
    }
}

/// 16進の数字の1桁 (小文字)。
pub open spec fn hex_digit(d: nat) -> u8 {
    if d < 10 {
        (b'0' + d) as u8
    } else {
        (b'a' + d - 10) as u8
    }
}

/// 0 以上の整数の16進の数字。小文字で、桁を埋めない。
pub open spec fn hex(n: nat) -> Seq<u8>
    decreases n,
{
    if n < 16 {
        seq![hex_digit(n)]
    } else {
        hex(n / 16) + seq![hex_digit(n % 16)]
    }
}

/// `String.show` が1バイトを書く形。規則は文字の単位で決めてある (docs/spec/declarations.md の「Prelude のクラス」)。
/// UTF-8 の先頭と続きのバイトはどれも 0x80 以上なので、正しい UTF-8 ではバイトの単位で書いても同じ結果になる。
pub open spec fn escape_byte(c: u8) -> Seq<u8> {
    if c == b'"' {
        seq![b'\\', b'"']
    } else if c == b'\\' {
        seq![b'\\', b'\\']
    } else if c == b'\n' {
        seq![b'\\', b'n']
    } else if c == b'\t' {
        seq![b'\\', b't']
    } else if c == b'\r' {
        seq![b'\\', b'r']
    } else if c == 0 {
        seq![b'\\', b'0']
    } else if c < 0x20 || c == 0x7f {
        seq![b'\\', b'u', b'{'] + hex(c as nat) + seq![b'}']
    } else {
        seq![c]
    }
}

/// 囲みの `"` の間に書くバイト列。
pub open spec fn escape_body(s: Seq<u8>) -> Seq<u8>
    decreases s.len(),
{
    if s.len() == 0 {
        Seq::empty()
    } else {
        escape_body(s.drop_last()) + escape_byte(s.last())
    }
}

/// `String.show` の結果。
pub open spec fn escape(s: Seq<u8>) -> Seq<u8> {
    seq![b'"'] + escape_body(s) + seq![b'"']
}

/// バイト列を `<[u8] as Ord>::cmp` で比べる。結果が `lex` に一致することは信頼する (信頼の一覧の T5)。vstd は slice の
/// `Ord` に約束を持たず、`assume_specification` は総称の `<[T] as Ord>::cmp` にしか置けないので、`u8` に限って包む。
#[verifier::external_body]
#[inline]
fn bytes_cmp(a: &[u8], b: &[u8]) -> (r: Ordering)
    ensures
        lex(a@, b@) == match r {
            Ordering::Less => LT,
            Ordering::Equal => EQ,
            Ordering::Greater => GT,
        },
{
    a.cmp(b)
}

/// ASCII のバイト列は正しい UTF-8 である。
proof fn lemma_ascii_utf8(s: Seq<u8>)
    requires
        forall|i: int| 0 <= i < s.len() ==> s[i] < 0x80,
    ensures
        valid_utf8(s),
    decreases s.len(),
{
    if s.len() > 0 {
        let rest = pop_first_scalar(s);
        assert(rest =~= s.subrange(1, s.len() as int));
        lemma_ascii_utf8(rest);
    }
}

/// 正しい UTF-8 の列で、ASCII のバイトの直前と直後は文字の境目である。`String.show` は、エスケープするバイト (どれも
/// ASCII) の前後と文字列の端で区間を切るので、区間の両端は文字の境目になる。
proof fn lemma_ascii_boundary(s: Seq<u8>, i: int)
    requires
        valid_utf8(s),
        0 <= i < s.len(),
        s[i] < 0x80,
    ensures
        is_char_boundary(s, i),
        is_char_boundary(s, i + 1),
{
    is_char_boundary_iff_not_is_continuation_byte(s, i);
    if i + 1 == s.len() {
        is_char_boundary_start_end_of_seq(s);
    } else {
        valid_utf8_split(s, i);
        let t = s.subrange(i, s.len() as int);
        assert(t =~= s.subrange(i, s.len() as int));
        assert(valid_first_scalar(t) && valid_utf8(pop_first_scalar(t)));
        let u = pop_first_scalar(t);
        assert(u =~= s.subrange(i + 1, s.len() as int));
        assert(valid_first_scalar(u));
        assert(u[0] == s[i + 1]);
        is_char_boundary_iff_not_is_continuation_byte(s, i + 1);
    }
}

/// `10^k`。
spec fn pow10(k: nat) -> nat
    decreases k,
{
    if k == 0 {
        1
    } else {
        10 * pow10((k - 1) as nat)
    }
}

/// `10^k` より小さい数は、`k` 桁以下である。
proof fn lemma_digits_len(n: nat, k: nat)
    requires
        1 <= k,
        n < pow10(k),
    ensures
        digits(n).len() <= k,
    decreases n,
{
    if n >= 10 {
        reveal_with_fuel(pow10, 2);
        let p = pow10((k - 1) as nat);
        assert(k >= 2);
        assert(n / 10 < p) by (nonlinear_arith)
            requires
                n < 10 * p,
        ;
        lemma_digits_len(n / 10, (k - 1) as nat);
    }
}

/// 10進の数字は ASCII の数字である。
proof fn lemma_digits_ascii(n: nat)
    ensures
        forall|j: int| 0 <= j < digits(n).len() ==> b'0' <= #[trigger] digits(n)[j] <= b'9',
    decreases n,
{
    let q = digits(n);
    if n >= 10 {
        lemma_digits_ascii(n / 10);
        let p = digits(n / 10);
        assert(q == p + seq![(b'0' + n % 10) as u8]);
        assert forall|j: int| 0 <= j < q.len() implies b'0' <= #[trigger] q[j] <= b'9' by {
            if j < p.len() {
                assert(q[j] == p[j]);
            }
        }
    } else {
        assert(q == seq![(b'0' + n) as u8]);
    }
}

/// `s` の前の `i + 1` バイトを書く形は、前の `i` バイトの形に `s[i]` の形を足したものである。
proof fn lemma_escape_step(s: Seq<u8>, i: int)
    requires
        0 <= i < s.len(),
    ensures
        escape_body(s.subrange(0, i + 1)) == escape_body(s.subrange(0, i)) + escape_byte(s[i]),
{
    let t = s.subrange(0, i + 1);
    assert(t.drop_last() =~= s.subrange(0, i));
}

/// エスケープしないバイトの区間 `[from, i)` は、そのまま写す。
proof fn lemma_escape_run(s: Seq<u8>, from: int, i: int)
    requires
        0 <= from <= i <= s.len(),
        forall|j: int| from <= j < i ==> escape_byte(#[trigger] s[j]) == seq![s[j]],
    ensures
        escape_body(s.subrange(0, i)) == escape_body(s.subrange(0, from)) + s.subrange(from, i),
    decreases i - from,
{
    if i > from {
        lemma_escape_run(s, from, i - 1);
        lemma_escape_step(s, i - 1);
        assert(s.subrange(from, i) =~= s.subrange(from, i - 1) + seq![s[i - 1]]);
    } else {
        assert(s.subrange(from, i) =~= Seq::<u8>::empty());
    }
}

/// 前の部分を書く形は、長い部分を書く形より長くない。
proof fn lemma_escape_len(s: Seq<u8>, j: int, k: int)
    requires
        0 <= j <= k <= s.len(),
    ensures
        escape_body(s.subrange(0, j)).len() <= escape_body(s.subrange(0, k)).len(),
    decreases k - j,
{
    if k > j {
        lemma_escape_len(s, j, k - 1);
        lemma_escape_step(s, k - 1);
    }
}

/// ASCII のバイト列を `&str` として借りる。
fn ascii_str(bytes: &[u8]) -> (s: &str)
    requires
        forall|j: int| 0 <= j < bytes@.len() ==> bytes@[j] < 0x80,
    ensures
        s.spec_bytes() == bytes@,
{
    proof {
        lemma_ascii_utf8(bytes@);
    }
    unsafe { str::from_utf8_unchecked(bytes) }
}

/// 16進の数字の1桁を書く。
fn hex_char(d: u8) -> (r: u8)
    requires
        d < 16,
    ensures
        r == hex_digit(d as nat),
        r < 0x80,
{
    if d < 10 {
        b'0' + d
    } else {
        b'a' + (d - 10)
    }
}

/// `escape_byte(c)` を `buf` の先頭に書き、その長さを返す。エスケープしないバイトなら何も書かずに 0 を返す。
fn escape_into(c: u8, buf: &mut [u8; 6]) -> (k: usize)
    ensures
        k == 0 <==> escape_byte(c) == seq![c],
        k > 0 ==> {
            &&& c < 0x80
            &&& k <= 6
            &&& final(buf)@.subrange(0, k as int) == escape_byte(c)
            &&& forall|j: int| 0 <= j < k ==> final(buf)@[j] < 0x80
        },
{
    let named = if c == b'"' {
        b'"'
    } else if c == b'\\' {
        b'\\'
    } else if c == b'\n' {
        b'n'
    } else if c == b'\t' {
        b't'
    } else if c == b'\r' {
        b'r'
    } else if c == 0 {
        b'0'
    } else {
        0
    };
    if named != 0 {
        buf[0] = b'\\';
        buf[1] = named;
        proof {
            assert(buf@.subrange(0, 2) =~= escape_byte(c));
        }
        return 2;
    }
    if c < 0x20 || c == 0x7f {
        buf[0] = b'\\';
        buf[1] = b'u';
        buf[2] = b'{';
        if c < 16 {
            buf[3] = hex_char(c);
            buf[4] = b'}';
            proof {
                assert(hex(c as nat) == seq![hex_digit(c as nat)]);
                assert(buf@.subrange(0, 5) =~= escape_byte(c));
            }
            5
        } else {
            buf[3] = hex_char(c / 16);
            buf[4] = hex_char(c % 16);
            buf[5] = b'}';
            proof {
                reveal_with_fuel(hex, 2);
                assert(hex(c as nat) =~= seq![
                    hex_digit((c / 16) as nat),
                    hex_digit((c % 16) as nat),
                ]);
                assert(buf@.subrange(0, 6) =~= escape_byte(c));
            }
            6
        }
    } else {
        0
    }
}

/// `escape_byte(c)` の長さ。
fn escape_len(c: u8) -> (k: usize)
    ensures
        k == escape_byte(c).len(),
{
    if c == b'"' || c == b'\\' || c == b'\n' || c == b'\t' || c == b'\r' || c == 0 {
        2
    } else if c < 0x10 {
        proof {
            assert(hex(c as nat) == seq![hex_digit(c as nat)]);
        }
        5
    } else if c < 0x20 || c == 0x7f {
        proof {
            reveal_with_fuel(hex, 2);
        }
        6
    } else {
        1
    }
}

/// `bytes[from..]` で最初にエスケープするバイトの位置 (なければ長さ) と、そのエスケープを `buf` に書いた長さを返す。
fn next_escape(bytes: &[u8], from: usize, buf: &mut [u8; 6]) -> (r: (usize, usize))
    requires
        from <= bytes@.len(),
    ensures
        from <= r.0 <= bytes@.len(),
        forall|j: int| from <= j < r.0 ==> escape_byte(#[trigger] bytes@[j]) == seq![bytes@[j]],
        r.0 < bytes@.len() ==> {
            &&& 0 < r.1 <= 6
            &&& bytes@[r.0 as int] < 0x80
            &&& final(buf)@.subrange(0, r.1 as int) == escape_byte(bytes@[r.0 as int])
            &&& forall|j: int| 0 <= j < r.1 ==> final(buf)@[j] < 0x80
        },
{
    let mut i = from;
    while i < bytes.len()
        invariant
            from <= i <= bytes@.len(),
            forall|j: int| from <= j < i ==> escape_byte(#[trigger] bytes@[j]) == seq![bytes@[j]],
        decreases bytes@.len() - i,
    {
        let k = escape_into(bytes[i], buf);
        if k > 0 {
            return (i, k);
        }
        i += 1;
    }
    (i, 0)
}

/// `String.show` の2回目の走査。`"` を書いた `w` に、`text` を書く形を書き足す (閉じる `"` は呼ぶ側が書く)。区間の
/// 両端は、エスケープするバイト (どれも ASCII) の前後か文字列の端なので、文字の境目である。
fn write_escaped<M: Mode>(
    heap: &mut Heap<M>,
    w: Word,
    s: Word,
    len: usize,
    Ghost(h2): Ghost<Heap<M>>,
    Ghost(text): Ghost<Seq<u8>>,
    Ghost(c): Ghost<bool>,
)
    requires
        old(heap).wf(),
        old(heap).core.model@.building is None,
        old(heap).dummy == h2.dummy,
        old(heap).region == h2.region,
        old(heap).worker == h2.worker,
        old(heap).core.text_changed(h2.core, w.bits_spec()),
        old(heap).appendable(w),
        old(heap).is_string(w),
        old(heap).capacity(w) >= 2 + escape_body(text).len(),
        old(heap).text_readable(s),
        old(heap).text_of(s) == text,
        s.bits_spec() != w.bits_spec(),
        valid_utf8(text),
        len == text.len(),
        old(heap).text_of(w) == seq![b'"'],
        c ==> old(heap).consistent(),
    ensures
        final(heap).wf(),
        final(heap).core.model@.building is None,
        final(heap).same_tokens(*old(heap)),
        final(heap).dummy == h2.dummy,
        final(heap).region == h2.region,
        final(heap).worker == h2.worker,
        final(heap).core.text_changed(h2.core, w.bits_spec()),
        final(heap).appendable(w),
        final(heap).is_string(w),
        final(heap).capacity(w) >= 2 + escape_body(text).len(),
        final(heap).text_readable(s),
        final(heap).text_of(s) == text,
        final(heap).text_of(w) == seq![b'"'] + escape_body(text),
        c ==> final(heap).consistent(),
        forall|x: Word| #[trigger] final(heap).core.word_ok(x) == old(heap).core.word_ok(x),
{
    let mut buf = [0u8;6];
    let mut from: usize = 0;
    let ghost h0 = *heap;
    proof {
        is_char_boundary_start_end_of_seq(text);
        assert(text.subrange(0, 0) =~= Seq::<u8>::empty());
        assert(heap.text_of(w) =~= seq![b'"'] + escape_body(text.subrange(0, 0)));
        assert(text.subrange(0, len as int) =~= text);
    }
    while from < len
        invariant
            heap.wf(),
            heap.core.model@.building is None,
            heap.same_tokens(h0),
            heap.dummy == h2.dummy,
            heap.region == h2.region,
            heap.worker == h2.worker,
            heap.core.text_changed(h2.core, w.bits_spec()),
            heap.appendable(w),
            heap.is_string(w),
            heap.capacity(w) >= 2 + escape_body(text).len(),
            heap.text_readable(s),
            heap.text_of(s) == text,
            s.bits_spec() != w.bits_spec(),
            valid_utf8(text),
            len == text.len(),
            text.subrange(0, len as int) == text,
            from <= len,
            is_char_boundary(text, from as int),
            heap.text_of(w) == seq![b'"'] + escape_body(text.subrange(0, from as int)),
            c ==> heap.consistent(),
            forall|x: Word| #[trigger] heap.core.word_ok(x) == h0.core.word_ok(x),
        decreases len - from,
    {
        let ghost before = *heap;
        let (i, k) = next_escape(heap.string_text(s).as_bytes(), from, &mut buf);
        proof {
            lemma_escape_run(text, from as int, i as int);
            if i < len {
                lemma_ascii_boundary(text, i as int);
            } else {
                is_char_boundary_start_end_of_seq(text);
            }
            lemma_escape_len(text, i as int, len as int);
        }
        if i > from {
            heap.append_part(w, s, from, i);
            proof {
                lemma_text_trans(heap.core, before.core, h2.core, w.bits_spec());
                assert(heap.text_of(s) == before.text_of(s));
            }
        }
        let ghost mid = *heap;
        proof {
            assert(heap.text_of(w) =~= seq![b'"'] + escape_body(text.subrange(0, i as int)));
        }
        if i < len {
            let e = ascii_str(vstd::slice::slice_subrange(buf.as_slice(), 0, k));
            proof {
                lemma_escape_step(text, i as int);
                lemma_escape_len(text, i + 1, len as int);
            }
            heap.append_str(w, e);
            proof {
                lemma_text_trans(heap.core, mid.core, h2.core, w.bits_spec());
                assert(heap.text_of(s) == mid.text_of(s));
                assert(heap.text_of(w) =~= seq![b'"'] + escape_body(text.subrange(0, i + 1)));
            }
            from = i + 1;
        } else {
            from = len;
        }
    }
}

/// その場の追記を続けても、変わるのはブロック `h` の語とバイト列だけである。
pub(super) proof fn lemma_text_trans(c2: Core, c1: Core, c0: Core, h: usize)
    requires
        c1.text_changed(c0, h),
        c2.text_changed(c1, h),
    ensures
        c2.text_changed(c0, h),
{
}

/// `string_push` の変化 (`pushed`)。`w` が `left` ならその場で足したので外の参照の数は変わらず、そうでなければ `left`
/// の参照を1つ手放して、前に生きていなかった `w` を作った。
pub(super) proof fn lemma_pushed_moved<M: Mode>(h1: Heap<M>, h0: Heap<M>, left: Word, w: Word)
    requires
        h0.consistent(),
        h0.settleable(),
        h1.consistent(),
        h1.pushed(h0, left, w),
        left.is_ptr_spec(),
    ensures
        moved(
            h1,
            h0,
            |x: usize| points(w, x) - points(left, x),
            Set::empty().insert(left.bits_spec()),
        ),
        w != left ==> !live(h0, w.bits_spec()),
        live(h1, w.bits_spec()),
        w.is_ptr_spec(),
        h1.owned(w),
{
    h1.lemma_pushed(h0, left, w);
    let v0 = h0.core.settled_view();
    let d = |x: usize| points(w, x) - points(left, x);
    let hw = w.bits_spec();
    h0.core.lemma_settled_view();
    if w != left && h0.core.region_has(hw) {
        h1.core.lemma_apart(hw);
    }
    assert forall|b: usize|
        #![trigger live(h0, b)]
        live(h0, b) && (v0.rc_of(b) == 0 || v0.ext(b) + d(b) >= 1) implies {
        &&& live(h1, b)
        &&& h1.core.block(b) == h0.core.block(b)
        &&& v0.rc_of(b) != 0 ==> h1.core.settled_view().ext(b) == v0.ext(b) + d(b)
    } by {
        if !v0.dom().contains(b) {
            v0.lemma_region_at(b);
            h1.core.lemma_apart(b);
        }
        if w != left {
            assert(b != hw);
        }
    }
}

/// 文字列のその場の追記の変化 (`text_changed`)。ブロック `h` の語とバイト列のほかは変わらない。
pub(super) proof fn lemma_text_moved<M: Mode>(h1: Heap<M>, h0: Heap<M>, h: usize)
    requires
        h0.core.model@.building is None,
        h1.core.text_changed(h0.core, h),
        h1.dummy == h0.dummy,
        h1.region == h0.region,
        h1.worker == h0.worker,
    ensures
        moved(h1, h0, |x: usize| 0int, Set::empty().insert(h)),
        no_free(h1, h0),
        no_alloc(h1, h0),
        exact(h1, h0, |x: usize| 0int),
{
    assert forall|b: usize| #[trigger] live(h1, b) == live(h0, b) by {}
    assert forall|b: usize| live(h0, b) && live(h1, b) && b != h implies #[trigger] h1.core.perm(b)
        == h0.core.perm(b) by {
        if h0.core.dom().contains(b) {
            assert(h1.core.perms@[b] == h0.core.perms@[b]);
        }
    }
}

/// `Show Char` が1文字を書く形 (囲みの `'` は含まない)。規則は文字の単位で決めてある (docs/spec/declarations.md の
/// 「Prelude のクラス」)。`String.show` と違い、`'` を `\'` と書き、`"` はそのまま写す。
pub open spec fn escape_char(c: u32) -> Seq<u8> {
    if c == 0x27 {
        seq![b'\\', b'\'']
    } else if c == 0x22 {
        seq![b'"']
    } else if c < 0x80 {
        escape_byte(c as u8)
    } else {
        encode_scalar(c)
    }
}

/// `Show Char` の `show` の結果。
pub open spec fn char_show_spec(c: u32) -> Seq<u8> {
    seq![b'\''] + escape_char(c) + seq![b'\'']
}

/// `Show Char` の `display` の結果。その1文字の UTF-8 である。
pub open spec fn char_display_spec(c: u32) -> Seq<u8> {
    encode_scalar(c)
}

/// スカラー値 `c` の UTF-8 を `buf` の先頭に書き、その長さを返す。
fn encode_char(c: u32, buf: &mut [u8; 4]) -> (k: usize)
    requires
        vstd::utf8::is_scalar(c),
    ensures
        k == encode_scalar(c).len(),
        final(buf)@.subrange(0, k as int) == encode_scalar(c),
{
    if c < 0x80 {
        buf[0] = (c & 0x7F) as u8;
        proof {
            assert(buf@.subrange(0, 1) =~= encode_scalar(c));
        }
        1
    } else if c < 0x800 {
        buf[0] = 0xC0 | ((c >> 6) & 0x1F) as u8;
        buf[1] = 0x80 | (c & 0x3F) as u8;
        proof {
            assert(buf@.subrange(0, 2) =~= encode_scalar(c));
        }
        2
    } else if c < 0x10000 {
        buf[0] = 0xE0 | ((c >> 12) & 0x0F) as u8;
        buf[1] = 0x80 | ((c >> 6) & 0x3F) as u8;
        buf[2] = 0x80 | (c & 0x3F) as u8;
        proof {
            assert(buf@.subrange(0, 3) =~= encode_scalar(c));
        }
        3
    } else {
        buf[0] = 0xF0 | ((c >> 18) & 0x7) as u8;
        buf[1] = 0x80 | ((c >> 12) & 0x3F) as u8;
        buf[2] = 0x80 | ((c >> 6) & 0x3F) as u8;
        buf[3] = 0x80 | (c & 0x3F) as u8;
        proof {
            assert(buf@.subrange(0, 4) =~= encode_scalar(c));
        }
        4
    }
}

/// スカラー値の UTF-8 は正しい UTF-8 である。
proof fn lemma_scalar_utf8(c: u32)
    requires
        vstd::utf8::is_scalar(c),
    ensures
        valid_utf8(encode_scalar(c)),
{
    let ch = c as char;
    assert(ch as u32 == c);
    assert(seq![ch].drop_first() =~= Seq::<char>::empty());
    assert(encode_utf8(Seq::<char>::empty()) =~= Seq::<u8>::empty());
    assert(encode_utf8(seq![ch]) =~= encode_scalar(c) + Seq::<u8>::empty());
    assert(encode_utf8(seq![ch]) =~= encode_scalar(c));
    encode_utf8_valid_utf8(seq![ch]);
}

/// `escape_char(c)` は正しい UTF-8 である。
proof fn lemma_escape_char_utf8(c: u32)
    requires
        vstd::utf8::is_scalar(c),
        c < 0x80 ==> forall|j: int| 0 <= j < escape_char(c).len() ==> escape_char(c)[j] < 0x80,
    ensures
        valid_utf8(escape_char(c)),
{
    if c >= 0x80 {
        lemma_scalar_utf8(c);
    } else {
        lemma_ascii_utf8(escape_char(c));
    }
}

/// `escape_char(c)` を `body` の先頭に書き、その長さを返す。
fn escape_char_into(c: u32, body: &mut [u8; 6]) -> (k: usize)
    requires
        vstd::utf8::is_scalar(c),
    ensures
        k == escape_char(c).len(),
        k <= 6,
        final(body)@.subrange(0, k as int) == escape_char(c),
        c < 0x80 ==> forall|j: int| 0 <= j < k ==> final(body)@[j] < 0x80,
{
    if c >= 0x80 {
        let mut tmp = [0u8;4];
        let k = encode_char(c, &mut tmp);
        let mut i = 0;
        while i < k
            invariant
                k <= 4,
                i <= k,
                tmp@.subrange(0, k as int) == encode_scalar(c),
                forall|j: int| 0 <= j < i ==> body@[j] == tmp@[j],
            decreases k - i,
        {
            body[i] = tmp[i];
            i += 1;
        }
        proof {
            assert(body@.subrange(0, k as int) =~= tmp@.subrange(0, k as int));
        }
        k
    } else if c == 0x27 {
        body[0] = b'\\';
        body[1] = b'\'';
        proof {
            assert(body@.subrange(0, 2) =~= escape_char(c));
            assert(body@[0] < 0x80 && body@[1] < 0x80);
        }
        2
    } else if c == 0x22 {
        body[0] = b'"';
        proof {
            assert(body@.subrange(0, 1) =~= escape_char(c));
            assert(body@[0] < 0x80);
        }
        1
    } else {
        let mut esc = [0u8;6];
        let k = escape_into(c as u8, &mut esc);
        if k == 0 {
            body[0] = c as u8;
            proof {
                assert(body@.subrange(0, 1) =~= escape_char(c));
                assert(body@[0] < 0x80);
            }
            1
        } else {
            let mut i = 0;
            while i < k
                invariant
                    k <= 6,
                    i <= k,
                    esc@.subrange(0, k as int) == escape_byte(c as u8),
                    forall|j: int| 0 <= j < k ==> esc@[j] < 0x80,
                    forall|j: int| 0 <= j < i ==> body@[j] == esc@[j],
                decreases k - i,
            {
                body[i] = esc[i];
                i += 1;
            }
            proof {
                assert(body@.subrange(0, k as int) =~= esc@.subrange(0, k as int));
                assert forall|j: int| 0 <= j < k implies body@[j] < 0x80 by {
                    assert(body@[j] == esc@[j]);
                }
            }
            k
        }
    }
}

impl<M: Mode> Ctx<M> {
    /// `w` は種類 `kind` の物体を指す。
    pub open spec fn is_body(&self, w: Word, kind: Kind) -> bool {
        w.is_ptr_spec() && self.heap_spec().kind_of(w) == kind
    }

    /// `w` は文字列の物体を指す。
    pub open spec fn is_string(&self, w: Word) -> bool {
        self.is_body(w, Kind::String)
    }

    /// `w` はバイト列の物体を指す。
    pub open spec fn is_bytes(&self, w: Word) -> bool {
        self.is_body(w, Kind::Bytes)
    }

    /// 文字列 `w` のバイト列。
    pub open spec fn text_of(&self, w: Word) -> Seq<u8> {
        self.heap_spec().text_of(w)
    }

    /// 文字列 `a` と `b` を借りる extern の前提。呼ぶ側が両方の参照を持つ。同じ物体でも、借りるだけで手放さないので
    /// 参照は1つでよい。
    pub open spec fn lends_strings(&self, a: Word, b: Word) -> bool {
        self.lends(a, b, Kind::String)
    }

    /// 種類 `kind` の物体 `a` と `b` を借りる extern の前提。`lends_strings` と同じ形である。
    pub open spec fn lends(&self, a: Word, b: Word, kind: Kind) -> bool {
        self.is_body(a, kind) && self.is_body(b, kind) && self.holds(a) && self.holds(b)
    }

    /// 文字列 `a` と `b` を受ける extern の前提。呼ぶ側が両方の参照を1つずつ持つ (同じ物体なら2つ持つ)。
    pub open spec fn holds_strings(&self, a: Word, b: Word) -> bool {
        self.holds_pair(a, b, Kind::String)
    }

    /// 種類 `kind` の物体 `a` と `b` の参照を、呼ぶ側が1つずつ持つ (同じ物体なら2つ持つ)。
    pub open spec fn holds_pair(&self, a: Word, b: Word, kind: Kind) -> bool {
        self.is_body(a, kind) && self.is_body(b, kind) && self.holds_args(seq![a, b])
    }

    /// `==`。両辺を借りる。呼ぶ側の参照の数は変わらない。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`a` と `b` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を
    /// 1つずつ持つ (同じ物体なら2つ持つ)。速いモードでは、どちらも文字列である (検査モードは確かめ、違えば `WrongKind`
    /// を返す)。
    pub unsafe fn string_eq(&mut self, a: Word, b: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            old(self).heap_spec().word_in(b),
            !M::CHECKED ==> old(self).lends_strings(a, b),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).lends_strings(a, b) ==> r is Ok || (r matches Err(
                Fault::Heap(e),
            ) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).lends_strings(a, b) && r is Ok ==> {
                &&& r == Ok::<i64, Fault>(
                    if old(self).text_of(a) == old(self).text_of(b) {
                        TRUE
                    } else {
                        FALSE
                    },
                )
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        self.compare(a, b, false, Kind::String)
    }

    /// `!=`。`string_eq` と同じ約束で、結果を逆にする。
    ///
    /// # Safety
    /// `string_eq` と同じ。
    pub unsafe fn string_ne(&mut self, a: Word, b: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            old(self).heap_spec().word_in(b),
            !M::CHECKED ==> old(self).lends_strings(a, b),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).lends_strings(a, b) ==> r is Ok || (r matches Err(
                Fault::Heap(e),
            ) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).lends_strings(a, b) && r is Ok ==> {
                &&& r == Ok::<i64, Fault>(
                    if old(self).text_of(a) == old(self).text_of(b) {
                        FALSE
                    } else {
                        TRUE
                    },
                )
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let eq = self.compare(a, b, false, Kind::String)?;
        Ok(TRUE - eq)
    }

    /// `compare`。`Ordering` のタグを返す。`string_eq` と同じく、両辺を借りる。
    ///
    /// # Safety
    /// `string_eq` と同じ。
    pub unsafe fn string_compare(&mut self, a: Word, b: Word) -> (r: Result<i64, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            old(self).heap_spec().word_in(b),
            !M::CHECKED ==> old(self).lends_strings(a, b),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).lends_strings(a, b) ==> r is Ok || (r matches Err(
                Fault::Heap(e),
            ) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).lends_strings(a, b) && r is Ok ==> {
                &&& r == Ok::<i64, Fault>(lex(old(self).text_of(a), old(self).text_of(b)))
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        self.compare(a, b, true, Kind::String)
    }

    /// 種類 `kind` の物体 `a` と `b` を借りて比べる。`order` なら `lex` のタグを、そうでなければ等しいかの `Bool` の
    /// タグを返す。検査モードの検査は、状態を変える前に済ませる。
    #[inline]
    pub(super) fn compare(&mut self, a: Word, b: Word, order: bool, kind: Kind) -> (r: Result<
        i64,
        Fault,
    >)
        requires
            old(self).wf(),
            kind.has_body(),
            old(self).heap_spec().word_in(a),
            old(self).heap_spec().word_in(b),
            !M::CHECKED ==> old(self).lends(a, b, kind),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            r matches Ok(v) ==> (!order ==> v == TRUE || v == FALSE),
            old(self).held_ok() && old(self).lends(a, b, kind) ==> r is Ok || (r matches Err(
                Fault::Heap(e),
            ) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).lends(a, b, kind) && r is Ok ==> {
                &&& r == Ok::<i64, Fault>(
                    if order {
                        lex(old(self).text_of(a), old(self).text_of(b))
                    } else if old(self).text_of(a) == old(self).text_of(b) {
                        TRUE
                    } else {
                        FALSE
                    },
                )
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.lends(a, b, kind);
        if M::CHECKED && !(a.is_ptr() && b.is_ptr()) {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let ha = self.heap.header_of(a);
        let hb = self.heap.header_of(b);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(a);
            h1.lemma_text_spec(b);
            s0.heap.lemma_text_spec(a);
            s0.heap.lemma_text_spec(b);
            if c {
                s0.lemma_settled_lent(h1, a, b, kind);
            }
        }
        if let Err(e) = self.heap.checked_body(ha, kind) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        if let Err(e) = self.heap.checked_body(hb, kind) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let r = {
            let x = self.heap.body_of(a);
            let y = self.heap.body_of(b);
            if order {
                match bytes_cmp(x, y) {
                    Ordering::Less => LT,
                    Ordering::Equal => EQ,
                    Ordering::Greater => GT,
                }
            } else if x.len() == y.len() && x == y {
                proof {
                    assert(x@ =~= y@);
                }
                TRUE
            } else {
                proof {
                    if x@.len() == y@.len() && x@ == y@ {
                        assert(x == y);
                    }
                }
                FALSE
            }
        };
        proof {
            assert(r == if order {
                lex(h1.text_of(a), h1.text_of(b))
            } else if h1.text_of(a) == h1.text_of(b) {
                TRUE
            } else {
                FALSE
            });
        }
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_lent(s0, h1);
            }
        }
        Ok(r)
    }

    /// `++`。両辺の参照を受け取り、結果の参照を返す。左がほかに参照のない文字列で容量が足りれば、その場で足す
    /// (`Heap::string_concat`)。
    ///
    /// # Safety
    /// `string_eq` と同じ。
    pub unsafe fn string_append(&mut self, a: Word, b: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(a),
            old(self).heap_spec().word_in(b),
            !M::CHECKED ==> old(self).holds_strings(a, b),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).holds_strings(a, b) ==> r is Ok || (r matches Err(
                Fault::Heap(e),
            ) && chain_err(Err::<(), HeapError>(e))),
            r matches Ok(w) ==> final(self).is_string(w) && final(self).text_of(w) == old(
                self,
            ).text_of(a) + old(self).text_of(b),
            r matches Ok(w) ==> (old(self).held_ok() && old(self).holds_strings(a, b) ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(
                    *old(self),
                    |x: usize| points(w, x) - points(a, x) - points(b, x),
                    Set::empty().insert(a.bits_spec()),
                )
            }),
    {
        unsafe { self.append_bodies(a, b, Kind::String) }
    }

    /// 種類 `kind` の2つの物体の連結。`string_append` と `bytes_append` の本体である。
    ///
    /// # Safety
    /// `string_eq` と同じで、両辺は種類 `kind` の物体である。
    pub(super) unsafe fn append_bodies(&mut self, a: Word, b: Word, kind: Kind) -> (r: Result<
        Word,
        Fault,
    >)
        requires
            old(self).wf(),
            kind.has_body(),
            old(self).heap_spec().word_in(a),
            old(self).heap_spec().word_in(b),
            !M::CHECKED ==> old(self).holds_pair(a, b, kind),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).holds_pair(a, b, kind) ==> r is Ok || (r matches Err(
                Fault::Heap(e),
            ) && chain_err(Err::<(), HeapError>(e))),
            r matches Ok(w) ==> final(self).is_body(w, kind) && final(self).text_of(w) == old(
                self,
            ).text_of(a) + old(self).text_of(b),
            r matches Ok(w) ==> (old(self).held_ok() && old(self).holds_pair(a, b, kind) ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(
                    *old(self),
                    |x: usize| points(w, x) - points(a, x) - points(b, x),
                    Set::empty().insert(a.bits_spec()),
                )
            }),
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.holds_pair(a, b, kind);
        if M::CHECKED && !(a.is_ptr() && b.is_ptr()) {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        let copied = if M::CHECKED {
            // 写すバイトを数えるために、左が一意かと両辺の長さを先に読む。読む前に、検査モードの検査を済ませる
            // (docs/spec/object-model.md の「仕事の回数」)
            self.heap.settle();
            proof {
                lemma_settled_twice(s0.heap);
                assert(self.heap.words_kept(s0.heap));
                if c {
                    s0.lemma_settled_strings(self.heap, a, b, kind);
                }
            }
            if let Err(e) = self.heap.checked_pair(a, b, kind) {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            }
            let ha = self.heap.header_of(a);
            let right = self.heap.body_of(b).len() as u64;
            // 一意な左には、その場で足すか作り直すかによらず右の長さだけを数え、共有か不死の左には両辺を数える
            // (docs/spec/object-model.md の「仕事の回数」)
            if self.heap.core.rc_get(ha) == 1 {
                right
            } else {
                (self.heap.body_of(a).len() as u64).wrapping_add(right)
            }
        } else {
            0
        };
        let ghost h1 = self.heap;
        proof {
            h1.lemma_text_spec(a);
            s0.heap.lemma_text_spec(a);
            h1.lemma_text_spec(b);
            s0.heap.lemma_text_spec(b);
            if c && !M::CHECKED {
                s0.lemma_strings_concat_ok(a, b, kind);
            }
        }
        let w = match unsafe { self.heap.concat(a, b, kind) } {
            Ok(w) => w,
            Err(e) => {
                proof {
                    lemma_settled_twice(s0.heap);
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            },
        };
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(copied);
        }
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_appended(s0, h1, a, b, w, kind);
            }
        }
        Ok(w)
    }

    /// `Int.show`。`i` の10進の文字列を作り、呼ぶ側がその参照を持つ。
    pub fn int_show(&mut self, i: i64) -> (w: Word)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            final(self).is_string(w),
            final(self).text_of(w) == decimal(i as int),
            old(self).held_ok() ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        // `-9223372036854775808` がちょうど 20 バイトである
        let mut buf = [0u8;20];
        let neg = i < 0;
        // `i64::MIN` の絶対値は `i64` に収まらないので、`u64` で求める
        let mut n: u64 = if neg {
            (-(i + 1)) as u64 + 1
        } else {
            i as u64
        };
        let ghost m = n as nat;
        proof {
            assert(pow10(19) == 10_000_000_000_000_000_000nat) by (compute);
            lemma_digits_len(m, 19);
            lemma_digits_ascii(m);
        }
        let mut pos: usize = 20;
        while n >= 10
            invariant
                digits(m) == digits(n as nat) + buf@.subrange(pos as int, 20),
                digits(n as nat).len() + (20 - pos) == digits(m).len(),
                digits(m).len() <= 19,
                pos <= 20,
            decreases n,
        {
            let ghost before = buf@;
            pos -= 1;
            buf[pos] = b'0' + (n % 10) as u8;
            proof {
                assert(buf@.subrange(pos as int, 20) =~= seq![(b'0' + n % 10) as u8]
                    + before.subrange(pos + 1, 20));
            }
            n /= 10;
        }
        let ghost before = buf@;
        pos -= 1;
        buf[pos] = b'0' + n as u8;
        proof {
            assert(buf@.subrange(pos as int, 20) =~= seq![(b'0' + n) as u8] + before.subrange(
                pos + 1,
                20,
            ));
            assert(buf@.subrange(pos as int, 20) =~= digits(m));
        }
        if neg {
            let ghost before = buf@;
            pos -= 1;
            buf[pos] = b'-';
            proof {
                assert(buf@.subrange(pos as int, 20) =~= seq![b'-'] + before.subrange(pos + 1, 20));
            }
        }
        let bytes = vstd::slice::slice_subrange(buf.as_slice(), pos, 20);
        proof {
            assert(bytes@ =~= decimal(i as int));
            lemma_ascii_utf8(bytes@);
        }
        let text = unsafe { str::from_utf8_unchecked(bytes) };
        let ghost s0 = *self;
        let w = self.heap.alloc_string(text);
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(
                (20 - pos) as u64,
            );
        }
        proof {
            s0.heap.lemma_text_spec(w);
            self.heap.lemma_text_spec(w);
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() {
                self.lemma_made_ok(s0, w);
            }
        }
        w
    }

    /// `Show Char` の `show`。`c` を `'` で囲み、文字のリテラルとして読み戻せる形の文字列を作る。呼ぶ側がその参照を持つ。
    ///
    /// # Safety
    /// `c` は Unicode のスカラー値である (docs/spec/lir.md の「値の表現」)。`Char` の型の値はこの不変条件を満たす。
    pub unsafe fn char_show(&mut self, c: i64) -> (w: Word)
        requires
            old(self).wf(),
            crate::externs::is_scalar(c as int),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            final(self).is_string(w),
            final(self).text_of(w) == char_show_spec(c as u32),
            old(self).held_ok() ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let u = c as u32;
        let mut body = [0u8;6];
        let k = escape_char_into(u, &mut body);
        let mut buf = [0u8;8];
        buf[0] = b'\'';
        let mut i = 0;
        while i < k
            invariant
                k <= 6,
                i <= k,
                buf@[0] == b'\'',
                body@.subrange(0, k as int) == escape_char(u),
                forall|j: int| 0 <= j < i ==> buf@[1 + j] == body@[j],
            decreases k - i,
        {
            buf[1 + i] = body[i];
            i += 1;
        }
        buf[1 + k] = b'\'';
        let ghost text_seq = buf@.subrange(0, 2 + k as int);
        proof {
            assert(text_seq =~= char_show_spec(u));
            assert(u < 0x80 ==> forall|j: int|
                0 <= j < escape_char(u).len() ==> escape_char(u)[j] < 0x80) by {
                if u < 0x80 {
                    assert forall|j: int| 0 <= j < escape_char(u).len() implies escape_char(u)[j]
                        < 0x80 by {
                        assert(escape_char(u)[j] == body@.subrange(0, k as int)[j]);
                    }
                }
            }
            lemma_escape_char_utf8(u);
            let q = seq![b'\''];
            lemma_ascii_utf8(q);
            valid_utf8_concat(q, escape_char(u));
            valid_utf8_concat(q + escape_char(u), q);
        }
        let bytes = vstd::slice::slice_subrange(buf.as_slice(), 0, 2 + k);
        let text = unsafe { str::from_utf8_unchecked(bytes) };
        let ghost s0 = *self;
        let w = self.heap.alloc_string(text);
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(
                (2 + k) as u64,
            );
        }
        proof {
            s0.heap.lemma_text_spec(w);
            self.heap.lemma_text_spec(w);
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() {
                self.lemma_made_ok(s0, w);
            }
        }
        w
    }

    /// `Show Char` の `display`。`c` 1文字の UTF-8 の文字列を作り、呼ぶ側がその参照を持つ。
    ///
    /// # Safety
    /// `c` は Unicode のスカラー値である (docs/spec/lir.md の「値の表現」)。
    pub unsafe fn char_display(&mut self, c: i64) -> (w: Word)
        requires
            old(self).wf(),
            crate::externs::is_scalar(c as int),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            final(self).is_string(w),
            final(self).text_of(w) == char_display_spec(c as u32),
            old(self).held_ok() ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let u = c as u32;
        let mut buf = [0u8;4];
        let k = encode_char(u, &mut buf);
        proof {
            lemma_scalar_utf8(u);
        }
        let bytes = vstd::slice::slice_subrange(buf.as_slice(), 0, k);
        let text = unsafe { str::from_utf8_unchecked(bytes) };
        let ghost s0 = *self;
        let w = self.heap.alloc_string(text);
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(k as u64);
        }
        proof {
            s0.heap.lemma_text_spec(w);
            self.heap.lemma_text_spec(w);
            if M::CHECKED {
                self.lemma_words_kept(s0);
            }
            if s0.held_ok() {
                self.lemma_made_ok(s0, w);
            }
        }
        w
    }

    /// `String.push`。`s` の参照を受け取り、後ろに `c` の UTF-8 を足した文字列の参照を返す。`s` が一意で容量が
    /// 足りれば、その場で足す (`Heap::string_push`)。
    ///
    /// # Safety
    /// `copy_string` と同じ (`s` の参照は受け取る)。`c` は Unicode のスカラー値である (docs/spec/lir.md の「値の表現」)。
    pub unsafe fn string_push_char(&mut self, c: i64, s: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            crate::externs::is_scalar(c as int),
            !M::CHECKED ==> old(self).is_string(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_string(s) && old(self).holds(s) ==> r is Ok || (
            r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            r matches Ok(x) ==> final(self).is_string(x) && final(self).text_of(x) == old(
                self,
            ).text_of(s) + encode_scalar(c as u32),
            r matches Ok(x) ==> (old(self).held_ok() && old(self).is_string(s) && old(self).holds(s)
                ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(x)
                &&& final(self).evolved_by(
                    *old(self),
                    |y: usize| points(x, y) - points(s, y),
                    Set::empty().insert(s.bits_spec()),
                )
            }),
    {
        let ghost s0 = *self;
        let ghost ok = s0.held_ok() && s0.is_string(s) && s0.holds(s);
        if M::CHECKED && !s.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        let u = c as u32;
        let mut buf = [0u8;4];
        let k = encode_char(u, &mut buf);
        proof {
            lemma_scalar_utf8(u);
        }
        let bytes = vstd::slice::slice_subrange(buf.as_slice(), 0, k);
        let text = unsafe { str::from_utf8_unchecked(bytes) };
        self.heap.settle();
        let ghost h1 = self.heap;
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(s);
            s0.heap.lemma_text_spec(s);
            if ok {
                s0.lemma_settled_body(h1, s, Kind::String);
                s0.lemma_settled_push_ok(h1, s, Kind::String);
            }
        }
        let copied = if M::CHECKED {
            // 写すバイトを数えるために、一意かと長さを先に読む。読む前に検査モードの検査を済ませる
            // (docs/spec/object-model.md の「仕事の回数」)
            let hs = self.heap.header_of(s);
            if let Err(e) = self.heap.checked_body(hs, Kind::String) {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            }
            if self.heap.core.rc_get(hs) == 1 {
                k as u64
            } else {
                (self.heap.body_of(s).len() as u64).wrapping_add(k as u64)
            }
        } else {
            0
        };
        let x = match unsafe { self.heap.string_push(s, text) } {
            Ok(x) => x,
            Err(e) => {
                proof {
                    self.lemma_words_kept(s0);
                }
                return Err(Fault::Heap(e));
            },
        };
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(copied);
        }
        proof {
            assert(text.spec_bytes() == encode_scalar(u));
            self.heap.lemma_text_spec(x);
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if ok {
                self.lemma_pushed_ok(s0, h1, seq![s], s, x, Kind::String);
            }
        }
        Ok(x)
    }

    /// `Show Float` の `show`。Python の `repr` と同じ形の文字列を作り、呼ぶ側がその参照を持つ。書式は T12 の
    /// `float_repr` が作る (docs/spec/object-model.md の「`unsafe` の核」)。
    pub fn float_show(&mut self, x: f64) -> (w: Word)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            final(self).is_string(w),
            old(self).held_ok() ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let text = float_repr(x);
        self.fresh_string(text.as_str())
    }

    /// `Float.to_fixed`。小数点以下 `digits` 桁の文字列を作り、呼ぶ側がその参照を持つ。桁数が 0 から 100 の外なら、
    /// 何も作らずに誤りを返す。書式は T12 の `float_fixed` が作る。
    // 範囲を比較で書くのは、vstd の `Range::contains` の約束を信頼せずに済むようにするためである
    #[allow(clippy::manual_range_contains)]
    pub fn float_to_fixed(&mut self, digits: i64, x: f64) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> *final(self) == *old(self) && r == Err::<Word, Fault>(
                Fault::Arith(ArithFault::FixedDigits(digits)),
            ),
            r matches Ok(w) ==> final(self).is_string(w),
            r matches Ok(w) ==> (old(self).held_ok() ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            }),
    {
        if digits < 0 || digits > 100 {
            return Err(Fault::Arith(ArithFault::FixedDigits(digits)));
        }
        let text = float_fixed(digits as usize, x);
        Ok(self.fresh_string(text.as_str()))
    }

    /// `Float.parse` の読み。`s` を借りて、読めた値か、読めなければ `None` を返す。読みは T12 の
    /// `float_parse` がする。
    ///
    /// # Safety
    /// `copy_string` と同じ。
    pub unsafe fn string_to_float(&mut self, s: Word) -> (r: Result<Option<f64>, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).is_string(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Ok || (r matches Err(Fault::Heap(_))),
            (r matches Err(Fault::Heap(_))) ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_string(s) && old(self).holds(s) ==> r is Ok || (
            r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            old(self).held_ok() && old(self).is_string(s) && old(self).holds(s) && r is Ok ==> {
                &&& final(self).held_ok()
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            },
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_string(s) && s0.holds(s);
        if M::CHECKED && !s.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hs = self.heap.header_of(s);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(s);
            s0.heap.lemma_text_spec(s);
            if c {
                s0.lemma_settled_body(h1, s, Kind::String);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let value = float_parse(self.heap.string_text(s));
        proof {
            if M::CHECKED {
                lemma_words_trans(self.heap, h1, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_lent(s0, h1);
            }
        }
        Ok(value)
    }

    /// `String.show`。`s` を `"` で囲み、エスケープした文字列を作る。`s` は借りて、結果の参照を返す。
    /// 1回目の走査で結果の長さを数えて1回だけ確保し、2回目の走査で、エスケープしないバイトの区間はまとめて、エスケープは
    /// 短い定数で、その場で書き足す。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。`s` は即値か、このヒープか付けた塊の生きている物体を指し、呼ぶ側がその参照を1つ
    /// 持つ。速いモードでは、`s` は文字列である (検査モードは確かめ、違えば `WrongKind` を返す)。
    pub unsafe fn string_show(&mut self, s: Word) -> (r: Result<Word, Fault>)
        requires
            old(self).wf(),
            old(self).heap_spec().word_in(s),
            !M::CHECKED ==> old(self).is_string(s) && old(self).holds(s),
        ensures
            final(self).wf(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            r is Err ==> (r matches Err(Fault::Heap(_)) && final(self).heap_spec().settled_heap()
                == old(self).heap_spec().settled_heap()),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).is_string(s) && old(self).holds(s) ==> r is Ok || (
            r matches Err(Fault::Heap(e)) && chain_err(Err::<(), HeapError>(e))),
            r matches Ok(w) ==> final(self).is_string(w) && final(self).text_of(w) == escape(
                old(self).text_of(s),
            ),
            r matches Ok(w) ==> (old(self).held_ok() && old(self).is_string(s) && old(self).holds(s)
                ==> {
                &&& final(self).held_ok()
                &&& final(self).holds(w)
                &&& final(self).evolved_by(*old(self), |x: usize| 0int, Set::empty())
            }),
    {
        let ghost s0 = *self;
        let ghost c = s0.held_ok() && s0.is_string(s) && s0.holds(s);
        if M::CHECKED && !s.is_ptr() {
            return Err(Fault::Heap(HeapError::WrongKind));
        }
        self.heap.settle();
        let ghost h1 = self.heap;
        let hs = self.heap.header_of(s);
        proof {
            lemma_settled_twice(s0.heap);
            assert(h1.words_kept(s0.heap));
            h1.lemma_text_spec(s);
            s0.heap.lemma_text_spec(s);
            if c {
                s0.lemma_settled_body(h1, s, Kind::String);
            }
        }
        if let Err(e) = self.heap.checked_body(hs, Kind::String) {
            proof {
                self.lemma_words_kept(s0);
            }
            return Err(Fault::Heap(e));
        }
        let ghost text = h1.text_of(s);
        // 1回目の走査。囲みの `"` の2バイトと、バイトごとの長さを足す
        let (len, n) = {
            let t = self.heap.string_text(s);
            proof {
                vstd::utf8::encode_utf8_valid_utf8(t@);
            }
            let bytes = t.as_bytes();
            let mut n: usize = 2;
            let mut i: usize = 0;
            while i < bytes.len()
                invariant
                    bytes@ == text,
                    i <= bytes@.len(),
                    n == 2 + escape_body(text.subrange(0, i as int)).len(),
                decreases bytes@.len() - i,
            {
                n =
                match n.checked_add(escape_len(bytes[i])) {
                    Some(m) => m,
                    None => too_large(),
                };
                proof {
                    lemma_escape_step(text, i as int);
                }
                i += 1;
            }
            proof {
                assert(text.subrange(0, i as int) =~= text);
            }
            (bytes.len(), n)
        };
        let w = self.heap.alloc_string_room(n);
        let ghost h2 = self.heap;
        proof {
            h2.lemma_text_spec(w);
            self.lemma_room_made(h1, s, w, Kind::String);
        }
        let quote = [b'"';1];
        let q = ascii_str(quote.as_slice());
        self.heap.append_str(w, q);
        proof {
            assert(self.heap.text_of(s) == text);
            assert(self.heap.text_of(w) =~= seq![b'"']);
        }
        write_escaped(&mut self.heap, w, s, len, Ghost(h2), Ghost(text), Ghost(c));
        let ghost before = self.heap;
        self.heap.append_str(w, q);
        let ghost h3 = self.heap;
        proof {
            lemma_text_trans(h3.core, before.core, h2.core, w.bits_spec());
            assert(self.heap.text_of(w) =~= escape(text));
            h3.lemma_text_spec(s);
            h3.lemma_text_spec(w);
            if c {
                s0.lemma_text_shown(h1, h2, h3, s, w, Kind::String);
            }
        }
        if M::CHECKED {
            self.stats.string_bytes_copied = self.stats.string_bytes_copied.wrapping_add(n as u64);
        }
        proof {
            self.heap.lemma_text_spec(w);
            if M::CHECKED {
                lemma_words_trans(h2, h1, s0.heap);
                assert(h3.words_kept(h2));
                lemma_words_trans(h3, h2, s0.heap);
                lemma_words_trans(self.heap, h3, s0.heap);
                self.lemma_words_kept(s0);
            }
            if c {
                self.lemma_shown_lent(s0, h1, h2, h3, s, w, Kind::String);
            }
        }
        Ok(w)
    }

    /// 借りるだけの extern の、モデルが合っている道。`settle` の後の `h1` から、何も手放していない。
    pub(super) proof fn lemma_lent(&self, s0: Ctx<M>, h1: Heap<M>)
        requires
            s0.held_ok(),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            self.heap == h1,
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.evolved_by(s0, |x: usize| 0int, Set::empty()),
    {
        let z = |x: usize| 0int;
        let none = Set::<usize>::empty();
        lemma_settle_moved(h1, s0.heap);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(seq![], x) + z(
            x,
        ) >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
        }
        self.lemma_holds_moved(s0, seq![], seq![], z, none);
        self.lemma_evv_kept(s0, z, none);
    }

    /// `String.slice`、`String.show`、`Bytes.slice` の、種類 `kind` の `s` を借りる道。確保と追記の後の `h3` から、
    /// 何も手放していない。
    pub(super) proof fn lemma_shown_lent(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        h2: Heap<M>,
        h3: Heap<M>,
        s: Word,
        w: Word,
        kind: Kind,
    )
        requires
            kind.has_body(),
            s0.held_ok(),
            s0.is_body(s, kind),
            s0.holds(s),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            h2.made(h1, w),
            h2.owned(w),
            h3.core.text_changed(h2.core, w.bits_spec()),
            h3.dummy == h2.dummy,
            h3.region == h2.region,
            h3.worker == h2.worker,
            h3.consistent(),
            h3.core.model@.building is None,
            self.heap == h3,
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(w),
            self.evolved_by(s0, |x: usize| 0int, Set::empty()),
    {
        let h0 = s0.heap;
        let z = |x: usize| 0int;
        let none = Set::<usize>::empty();
        s0.lemma_text_shown(h1, h2, h3, s, w, kind);
        lemma_settle_moved(h1, h0);
        lemma_moved_chain(h3, h1, h0, z, z, z, none, none, none);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(seq![], x) + z(
            x,
        ) >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
        }
        self.lemma_holds_moved(s0, seq![], seq![], z, none);
        self.lemma_evv_kept(s0, z, none);
        assert(s.bits_spec() != w.bits_spec());
        lemma_view_live(self.heap, w.bits_spec());
        self.lemma_fresh_holds(s0, w);
    }

    /// 種類 `kind` の物体を1つ受ける extern の前提から、`settle` の後のヒープ `h1` で、`s` を読めて手放せることを言う。
    pub(super) proof fn lemma_settled_body(&self, h1: Heap<M>, s: Word, kind: Kind)
        requires
            self.held_ok(),
            self.is_body(s, kind),
            self.holds(s),
            h1 == self.heap.settled_heap(),
            h1.consistent(),
        ensures
            held_word(h1, s),
            live(h1, s.bits_spec()),
            h1.core.state(s.bits_spec()) == BlockState::Live,
            h1.core.kind(s.bits_spec()) == kind,
            h1.text_of(s) == self.text_of(s),
            h1.core.rc_of(s.bits_spec()) != 0 ==> h1.core.ext(s.bits_spec()) >= 1,
            !M::CHECKED ==> h1.readable_as(s, kind),
    {
        let ws = seq![s];
        lemma_settle_moved(h1, self.heap);
        self.lemma_arg_counted(ws, 0);
        lemma_points_in(ws, s.bits_spec());
        lemma_arg_moved(h1, self.heap, |x: usize| 0int, Set::empty(), s);
        h1.lemma_text_spec(s);
        self.heap.lemma_text_spec(s);
    }

    /// 確保は、前に確かめた種類 `kind` の物体 `s` に触れない。
    pub(super) proof fn lemma_room_made(&self, h1: Heap<M>, s: Word, w: Word, kind: Kind)
        requires
            h1.wf(),
            h1.core.model@.building is None,
            h1.readable_as(s, kind),
            self.heap.grew(h1, w),
            self.heap.made(h1, w),
        ensures
            self.heap.readable_as(s, kind),
            self.heap.text_of(s) == h1.text_of(s),
            s.bits_spec() != w.bits_spec(),
            self.heap.core.has(s.bits_spec()),
            self.heap.core.block(s.bits_spec()) == h1.core.block(s.bits_spec()),
    {
        let b = s.bits_spec();
        h1.core.lemma_shape(b);
        if M::CHECKED {
            h1.core.lemma_code_state(b);
            reveal(Core::building_wf);
        } else {
            h1.core.lemma_readable(s);
            reveal(Core::building_wf);
        }
        assert(h1.core.settled_state(b) == BlockState::Live);
        assert(self.heap.grown_block(h1, b));
        h1.lemma_text_spec(s);
        self.heap.lemma_text_spec(s);
    }

    /// `String.slice`、`String.show`、`Bytes.slice` で、確保と追記の後の、モデルが合っている道。確保と追記は `s` に
    /// 触れないので、`s` はまだ持たれている。
    pub(super) proof fn lemma_text_shown(
        &self,
        h1: Heap<M>,
        h2: Heap<M>,
        h3: Heap<M>,
        s: Word,
        w: Word,
        kind: Kind,
    )
        requires
            kind.has_body(),
            self.held_ok(),
            self.is_body(s, kind),
            self.holds(s),
            h1 == self.heap.settled_heap(),
            h1.consistent(),
            h2.made(h1, w),
            h2.owned(w),
            h3.core.text_changed(h2.core, w.bits_spec()),
            h3.dummy == h2.dummy,
            h3.region == h2.region,
            h3.worker == h2.worker,
            h3.consistent(),
        ensures
            moved(h3, h1, |x: usize| 0int, Set::empty()),
            no_free(h3, h1),
            exact(h3, h1, |x: usize| 0int),
            !live(h1, w.bits_spec()),
            live(h1, s.bits_spec()),
            h3.core.state(s.bits_spec()) == BlockState::Live,
            h3.core.rc_of(s.bits_spec()) != 0 ==> h3.core.ext(s.bits_spec()) >= 1,
            held_word(h3, w),
            h3.core.ext(w.bits_spec()) == 1,
    {
        let z = |x: usize| 0int;
        let none = Set::<usize>::empty();
        let hw = w.bits_spec();
        self.lemma_settled_body(h1, s, kind);
        lemma_alloc_moved(h2, h1, w);
        lemma_text_moved(h3, h2, hw);
        lemma_moved_chain(h3, h2, h1, z, z, z, none, none.insert(hw), none);
        lemma_arg_moved(h3, h1, z, none, s);
        lemma_view_live(h3, s.bits_spec());
        assert(live(h2, hw));
        assert(h3.core.word_ok(w) == h2.core.word_ok(w));
    }

    /// 種類 `kind` の物体を2つ借りる extern の前提から、`settle` の後のヒープ `h1` で、2つを読めることを言う。
    pub(super) proof fn lemma_settled_lent(&self, h1: Heap<M>, a: Word, b: Word, kind: Kind)
        requires
            self.held_ok(),
            self.lends(a, b, kind),
            h1 == self.heap.settled_heap(),
            h1.consistent(),
        ensures
            held_word(h1, a),
            held_word(h1, b),
            live(h1, a.bits_spec()),
            live(h1, b.bits_spec()),
            h1.core.state(a.bits_spec()) == BlockState::Live,
            h1.core.state(b.bits_spec()) == BlockState::Live,
            h1.core.kind(a.bits_spec()) == kind,
            h1.core.kind(b.bits_spec()) == kind,
            h1.text_of(a) == self.text_of(a),
            h1.text_of(b) == self.text_of(b),
            !M::CHECKED ==> h1.readable_as(a, kind) && h1.readable_as(b, kind),
    {
        self.lemma_settled_body(h1, a, kind);
        self.lemma_settled_body(h1, b, kind);
        let h0 = self.heap;
        let z = |x: usize| 0int;
        lemma_settle_moved(h1, h0);
        lemma_arg_moved(h1, h0, z, Set::empty(), a);
        lemma_arg_moved(h1, h0, z, Set::empty(), b);
        h0.lemma_text_spec(a);
        h0.lemma_text_spec(b);
        h1.lemma_text_spec(a);
        h1.lemma_text_spec(b);
    }

    /// 種類 `kind` の物体を2つ受ける extern の前提から、`settle` の後のヒープ `h1` で、ヒープの操作の前提を言う。
    pub(super) proof fn lemma_settled_strings(&self, h1: Heap<M>, a: Word, b: Word, kind: Kind)
        requires
            kind.has_body(),
            self.held_ok(),
            self.holds_pair(a, b, kind),
            h1 == self.heap.settled_heap(),
            h1.consistent(),
        ensures
            h1.concat_ok(a, b, kind),
            h1.text_of(a) == self.text_of(a),
            h1.text_of(b) == self.text_of(b),
            !M::CHECKED ==> h1.readable_as(a, kind) && h1.readable_as(b, kind),
    {
        let h0 = self.heap;
        let ws = seq![a, b];
        let z = |x: usize| 0int;
        lemma_settle_moved(h1, h0);
        self.lemma_arg_counted(ws, 0);
        self.lemma_arg_counted(ws, 1);
        lemma_points_in(ws, a.bits_spec());
        lemma_arg_moved(h1, h0, z, Set::empty(), a);
        lemma_arg_moved(h1, h0, z, Set::empty(), b);
        h0.lemma_text_spec(a);
        h0.lemma_text_spec(b);
        h1.lemma_text_spec(a);
        h1.lemma_text_spec(b);
        if a.bits_spec() == b.bits_spec() && h1.rc_value(a) != 0 {
            assert(live(h0, a.bits_spec()));
        }
        h1.lemma_concat_ok(a, b, kind);
    }

    /// 速いモードで、種類 `kind` の物体を2つ受ける extern の前提から `Heap::concat` の前提を言う。
    pub(super) proof fn lemma_strings_concat_ok(&self, a: Word, b: Word, kind: Kind)
        requires
            kind.has_body(),
            self.held_ok(),
            self.holds_pair(a, b, kind),
        ensures
            self.heap.concat_ok(a, b, kind),
    {
        let h0 = self.heap;
        let ws = seq![a, b];
        self.lemma_arg_counted(ws, 0);
        self.lemma_arg_counted(ws, 1);
        lemma_points_in(ws, a.bits_spec());
        lemma_view_live(h0, a.bits_spec());
        lemma_view_live(h0, b.bits_spec());
        if a.bits_spec() == b.bits_spec() && h0.rc_value(a) != 0 {
            assert(live(h0, a.bits_spec()));
        }
        h0.lemma_concat_ok(a, b, kind);
    }

    /// 新しい物体 `w` を作っただけの変化の後、呼ぶ側は `w` の参照を `Ctx` と別に1つ持つ。
    pub(super) proof fn lemma_made_ok(&self, s0: Ctx<M>, w: Word)
        requires
            s0.held_ok(),
            self.heap.made(s0.heap, w),
            self.heap.owned(w),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(w),
            self.evolved_by(s0, |x: usize| 0int, Set::empty()),
    {
        let z = |x: usize| 0int;
        let none = Set::<usize>::empty();
        lemma_alloc_moved(self.heap, s0.heap, w);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(seq![], x) + z(
            x,
        ) >= self.need(seq![], x) by {}
        self.lemma_holds_moved(s0, seq![], seq![], z, none);
        self.lemma_evv_kept(s0, z, none);
        lemma_view_live(self.heap, w.bits_spec());
        self.lemma_fresh_holds(s0, w);
    }

    /// `string_append` と `bytes_append` の、モデルが合っている道。`h1` は `Heap::concat` の前の状態である。
    pub(super) proof fn lemma_appended(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        a: Word,
        b: Word,
        w: Word,
        kind: Kind,
    )
        requires
            kind.has_body(),
            s0.held_ok(),
            s0.holds_pair(a, b, kind),
            h1 == s0.heap || h1 == s0.heap.settled_heap(),
            h1.consistent(),
            h1.concat_ok(a, b, kind),
            self.heap.consistent(),
            self.heap.concatenated(h1, a, b, w),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(w),
            self.evolved_by(
                s0,
                |x: usize| points(w, x) - points(a, x) - points(b, x),
                Set::empty().insert(a.bits_spec()),
            ),
    {
        let h0 = s0.heap;
        let z = |x: usize| 0int;
        let d1 = |x: usize| points(w, x) - points(a, x);
        let db = |x: usize| -points(b, x);
        let d = |x: usize| points(w, x) - points(a, x) - points(b, x);
        let none = Set::<usize>::empty();
        let xa = none.insert(a.bits_spec());
        let ws = seq![a, b];
        let mid = self.heap.lemma_concatenated(h1, a, b, w);
        mid.lemma_pushed(h1, a, w);
        if h1 == h0 {
            lemma_moved_refl(h0);
        } else {
            lemma_settle_moved(h1, h0);
        }
        lemma_pushed_moved(mid, h1, a, w);
        lemma_decref_moved(self.heap, mid, b);
        lemma_moved_chain(self.heap, mid, h1, d1, db, d, xa, none, xa);
        lemma_moved_chain(self.heap, h1, h0, z, d, d, none, xa, xa);
        h0.lemma_text_spec(a);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(ws, x) + d(x)
            >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
            lemma_points_in(ws, x);
            assert(live(h0, x) ==> w == a || points(w, x) == 0);
        }
        self.lemma_holds_moved(s0, ws, seq![], d, xa);
        if w == a {
            assert forall|x: usize| #[trigger] self.need(seq![w], x) > 0 implies s0.need(ws, x) + d(
                x,
            ) >= self.need(seq![w], x) by {
                lemma_points_in(seq![w], x);
                lemma_points_in(ws, x);
            }
            s0.lemma_arg_counted(ws, 0);
            self.lemma_holds_moved(s0, ws, seq![w], d, xa);
        } else {
            lemma_view_live(self.heap, w.bits_spec());
            assert(self.heap.core.settled_state(w.bits_spec()) == BlockState::Live);
            assert(self.heap.core.kept_block(mid.core, w.bits_spec()));
            self.lemma_fresh_holds(s0, w);
        }
        // `a` は文字列なので、`evv` の節点でも evidence でも入口の表でもない
        lemma_listed_kept(self.heap, h0, d, xa, self.evv, self.max_evv);
    }

    /// 種類 `kind` の物体を1つ受けて手放す extern の前提から、`settle` の後のヒープ `h1` で `push_ok` を言う。
    pub(super) proof fn lemma_settled_push_ok(&self, h1: Heap<M>, w: Word, kind: Kind)
        requires
            kind.has_body(),
            self.held_ok(),
            self.is_body(w, kind),
            self.holds_args(seq![w]),
            h1 == self.heap.settled_heap(),
            h1.consistent(),
        ensures
            h1.push_ok(w, kind),
            h1.text_of(w) == self.text_of(w),
    {
        let h0 = self.heap;
        let ws = seq![w];
        let z = |x: usize| 0int;
        lemma_settle_moved(h1, h0);
        self.lemma_arg_counted(ws, 0);
        lemma_points_in(ws, w.bits_spec());
        lemma_arg_moved(h1, h0, z, Set::empty(), w);
        h0.lemma_text_spec(w);
        h1.lemma_text_spec(w);
        h1.lemma_push_ok(w, kind);
    }

    /// 種類 `kind` の物体 `left` を受けて同じ種類の `w` を返す extern の、モデルが合っている道。呼ぶ側は `ws` の
    /// 参照を持ち、その先頭が `left` で、ほかの語は借りただけである。`h1` は `settle` の後の状態である。
    pub(super) proof fn lemma_pushed_ok(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        ws: Seq<Word>,
        left: Word,
        w: Word,
        kind: Kind,
    )
        requires
            kind.has_body(),
            s0.held_ok(),
            s0.holds_args(ws),
            1 <= ws.len() <= 3,
            ws[0] == left,
            s0.is_body(left, kind),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            h1.settleable(),
            self.heap.consistent(),
            self.heap.pushed(h1, left, w),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(w),
            self.evolved_by(
                s0,
                |x: usize| points(w, x) - points(left, x),
                Set::empty().insert(left.bits_spec()),
            ),
    {
        let h0 = s0.heap;
        let z = |x: usize| 0int;
        let d = |x: usize| points(w, x) - points(left, x);
        let none = Set::<usize>::empty();
        let xa = none.insert(left.bits_spec());
        self.heap.lemma_pushed(h1, left, w);
        lemma_settle_moved(h1, h0);
        lemma_pushed_moved(self.heap, h1, left, w);
        lemma_moved_chain(self.heap, h1, h0, z, d, d, none, xa, xa);
        h0.lemma_text_spec(left);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(ws, x) + d(x)
            >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
            lemma_points_in(ws, x);
            assert(live(h0, x) ==> w == left || points(w, x) == 0);
        }
        self.lemma_holds_moved(s0, ws, seq![], d, xa);
        if w == left {
            assert forall|x: usize| #[trigger] self.need(seq![w], x) > 0 implies s0.need(ws, x) + d(
                x,
            ) >= self.need(seq![w], x) by {
                lemma_points_in(seq![w], x);
                lemma_points_in(ws, x);
            }
            s0.lemma_arg_counted(ws, 0);
            self.lemma_holds_moved(s0, ws, seq![w], d, xa);
        } else {
            lemma_view_live(self.heap, w.bits_spec());
            assert(self.heap.core.settled_state(w.bits_spec()) == BlockState::Live);
            self.lemma_fresh_holds(s0, w);
        }
        // `left` は文字列かバイト列なので、`evv` の節点でも evidence でも入口の表でもない
        lemma_listed_kept(self.heap, h0, d, xa, self.evv, self.max_evv);
    }

    /// 物体 `left` を受けて手放すだけで終わる extern の誤りの道 (範囲の外の位置)。呼ぶ側は `ws` の参照を持ち、その
    /// 先頭が `left` で、ほかの語は借りただけである。
    pub(super) proof fn lemma_dropped_ok(&self, s0: Ctx<M>, h1: Heap<M>, ws: Seq<Word>, left: Word)
        requires
            s0.held_ok(),
            s0.holds_args(ws),
            1 <= ws.len() <= 3,
            ws[0] == left,
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            self.heap.consistent(),
            self.heap.settled(),
            self.heap.evolved(h1, left, -1),
            self.heap.kept_live(h1),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.evolved_by(s0, |x: usize| -points(left, x), Set::empty()),
    {
        let h0 = s0.heap;
        let z = |x: usize| 0int;
        let d = |x: usize| -points(left, x);
        let none = Set::<usize>::empty();
        lemma_settle_moved(h1, h0);
        lemma_decref_moved(self.heap, h1, left);
        lemma_moved_chain(self.heap, h1, h0, z, d, d, none, none, none);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(ws, x) + d(x)
            >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
            lemma_points_in(ws, x);
        }
        self.lemma_holds_moved(s0, ws, seq![], d, none);
        self.lemma_evv_kept(s0, d, none);
    }

    /// 種類 `kind` の物体 `s` を受けて、中身を写した別の物体 `w` を返す extern (`Bytes.of_string`、`String.decode`) の、
    /// モデルが合っている道。確保と追記の後の `h3` から、`s` の参照を1つ手放した。
    pub(super) proof fn lemma_converted_ok(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        h2: Heap<M>,
        h3: Heap<M>,
        s: Word,
        w: Word,
        kind: Kind,
    )
        requires
            kind.has_body(),
            s0.held_ok(),
            s0.is_body(s, kind),
            s0.holds(s),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            h2.made(h1, w),
            h2.owned(w),
            h3.core.text_changed(h2.core, w.bits_spec()),
            h3.dummy == h2.dummy,
            h3.region == h2.region,
            h3.worker == h2.worker,
            h3.consistent(),
            h3.core.model@.building is None,
            self.heap.consistent(),
            self.heap.settled(),
            self.heap.evolved(h3, s, -1),
            self.heap.kept_live(h3),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(w),
            self.evolved_by(s0, |x: usize| -points(s, x), Set::empty()),
    {
        let h0 = s0.heap;
        let z = |x: usize| 0int;
        let d = |x: usize| -points(s, x);
        let none = Set::<usize>::empty();
        let ws = seq![s];
        s0.lemma_text_shown(h1, h2, h3, s, w, kind);
        lemma_settle_moved(h1, h0);
        lemma_decref_moved(self.heap, h3, s);
        lemma_moved_chain(self.heap, h3, h1, z, d, d, none, none, none);
        lemma_moved_chain(self.heap, h1, h0, z, d, d, none, none, none);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(ws, x) + d(x)
            >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
            lemma_points_in(ws, x);
        }
        self.lemma_holds_moved(s0, ws, seq![], d, none);
        self.lemma_evv_kept(s0, d, none);
        assert(s.bits_spec() != w.bits_spec());
        lemma_view_live(self.heap, w.bits_spec());
        self.lemma_fresh_holds(s0, w);
    }

    /// 新しい物体 `w` を作り、その場で書き足しただけの変化の後、呼ぶ側は `w` の参照を `Ctx` と別に1つ持つ。
    pub(super) proof fn lemma_made_filled(&self, s0: Ctx<M>, h2: Heap<M>, w: Word)
        requires
            s0.held_ok(),
            h2.made(s0.heap, w),
            h2.owned(w),
            self.heap.core.text_changed(h2.core, w.bits_spec()),
            self.heap.dummy == h2.dummy,
            self.heap.region == h2.region,
            self.heap.worker == h2.worker,
            self.heap.consistent(),
            self.heap.core.model@.building is None,
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.holds(w),
            self.evolved_by(s0, |x: usize| 0int, Set::empty()),
    {
        let h0 = s0.heap;
        let h3 = self.heap;
        let z = |x: usize| 0int;
        let none = Set::<usize>::empty();
        let hw = w.bits_spec();
        lemma_alloc_moved(h2, h0, w);
        lemma_text_moved(h3, h2, hw);
        lemma_moved_chain(h3, h2, h0, z, z, z, none, none.insert(hw), none);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(seq![], x) + z(
            x,
        ) >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
        }
        self.lemma_holds_moved(s0, seq![], seq![], z, none);
        self.lemma_evv_kept(s0, z, none);
        assert(live(h2, hw));
        assert(h3.core.word_ok(w) == h2.core.word_ok(w));
        lemma_view_live(self.heap, hw);
        self.lemma_fresh_holds(s0, w);
    }

    /// `compare` の、モデルが合っている道。`settle` の後の `h1` から `a` と `b` を手放した。
    pub(super) proof fn lemma_released(
        &self,
        s0: Ctx<M>,
        h1: Heap<M>,
        mid: Heap<M>,
        a: Word,
        b: Word,
    )
        requires
            s0.held_ok(),
            s0.holds_strings(a, b),
            h1 == s0.heap.settled_heap(),
            h1.consistent(),
            mid.consistent(),
            mid.core.model@.building is None,
            mid.evolved(h1, a, -1),
            mid.kept_live(h1),
            self.heap.consistent(),
            self.heap.core.model@.building is None,
            self.heap.evolved(mid, b, -1),
            self.heap.kept_live(mid),
            self.evv == s0.evv,
            self.yielding == s0.yielding,
            self.max_evv == s0.max_evv,
        ensures
            self.held_ok(),
            self.evolved_by(s0, |x: usize| -points(a, x) - points(b, x), Set::empty()),
    {
        let h0 = s0.heap;
        let z = |x: usize| 0int;
        let da = |x: usize| -points(a, x);
        let db = |x: usize| -points(b, x);
        let d = |x: usize| -points(a, x) - points(b, x);
        let none = Set::<usize>::empty();
        let ws = seq![a, b];
        lemma_settle_moved(h1, h0);
        lemma_decref_moved(mid, h1, a);
        lemma_decref_moved(self.heap, mid, b);
        lemma_moved_chain(self.heap, mid, h1, da, db, d, none, none, none);
        lemma_moved_chain(self.heap, h1, h0, z, d, d, none, none, none);
        assert forall|x: usize| #[trigger] self.need(seq![], x) > 0 implies s0.need(ws, x) + d(x)
            >= self.need(seq![], x) by {
            lemma_points_in(seq![], x);
            lemma_points_in(ws, x);
        }
        self.lemma_holds_moved(s0, ws, seq![], d, none);
        self.lemma_evv_kept(s0, d, none);
    }
}

} // verus!
