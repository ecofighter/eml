//! `Foreign` の物体 (docs/spec/object-model.md の「種類」)。ペイロードの先頭に破棄処理の表への参照を置き、その後に
//! Rust の値を置く。Verus は関数のポインタと `TypeId` を扱わないので、ここの関数は信頼の一覧の T3 である。どれも
//! ブロックの権限を受け、そのペイロードの中だけを読み書きする。表と値を書いた語を、検証したコードは読まない。種類
//! `Foreign` の物体は走査する語を持たず、`raw` と `set_raw` も `Foreign` の物体を断るためである。

use std::any::TypeId;
use std::marker::PhantomData;

use crate::meta::{BlockPerm, Header, slot_ptr};
use vstd::prelude::*;

verus! {

#[verifier::external]
pub(crate) struct VTable {
    /// 型の照らし合わせに使う。表の番地は型ごとに1つと限らないので、番地では比べない。
    type_id: fn () -> TypeId,
    finalize: unsafe fn (*mut u8),
}

#[verifier::external]
struct Table<T>(PhantomData<T>);

#[verifier::external]
unsafe fn drop_value<T>(value: *mut u8) {
    unsafe { value.cast::<T>().drop_in_place() }
}

/// `T` の値の語の数。`alloc_foreign` は、表の1語とこの数の語のペイロードを取る。
#[verifier::external_body]
#[inline]
pub(crate) fn value_words<T>() -> (r: usize)
    ensures
        r < 0x1000_0000_0000_0000,
{
    const { assert!(align_of::<T>() <= 8, "a foreign value must be at most word-aligned") };
    size_of::<T>().div_ceil(8)
}

/// `h` のペイロードに、破棄処理の表と値を書く。ペイロードの `cap` 語が表の1語と値に足りなければ止める。権限は欄の
/// 位置を保つ。
#[verifier::external_body]
#[inline]
pub(crate) fn write<T: Send + 'static>(
    h: *mut Header,
    cap: usize,
    value: T,
    Tracked(perm): Tracked<&mut BlockPerm>,
)
    requires
        forall|i: int| #[trigger] old(perm).slots.contains_key(i) <==> 0 <= i < cap,
    ensures
        final(perm).rc == old(perm).rc,
        final(perm).meta == old(perm).meta,
        final(perm).prefix == old(perm).prefix,
        final(perm).slots.dom() == old(perm).slots.dom(),
        forall|i: int|
            0 <= i < cap ==> (#[trigger] final(perm).slots[i]).ptr() == old(perm).slots[i].ptr(),
{
    assert!(value_words::<T>() < cap, "a foreign payload too small for its value");
    let table: &'static VTable = &Table::<T>::VTABLE;
    unsafe {
        slot_ptr(h, 0).cast::<&'static VTable>().write(table);
        slot_ptr(h, 1).cast::<T>().write(value);
    }
}

/// 値が `T` なら、その位置を返す。
#[verifier::external_body]
#[inline]
pub(crate) fn value<T: 'static>(h: *mut Header, Tracked(perm): Tracked<&BlockPerm>) -> Option<
    *mut T,
> {
    let table = unsafe { slot_ptr(h, 0).cast::<&'static VTable>().read() };
    ((table.type_id)() == TypeId::of::<T>()).then(|| slot_ptr(h, 1).cast())
}

/// 破棄処理と値の位置。物体の状態を替えたあとで呼べるように、先に読み出しておく。
#[verifier::external_body]
pub(crate) struct Finalizer {
    finalize: unsafe fn (*mut u8),
    value: *mut u8,
}

/// `Foreign` の物体 `h` の破棄処理と値の位置を、物体の状態を替える前に読み出しておく。
#[verifier::external_body]
#[inline]
pub(crate) fn finalizer_of(h: *mut Header, Tracked(perm): Tracked<&BlockPerm>) -> Finalizer {
    let table = unsafe { slot_ptr(h, 0).cast::<&'static VTable>().read() };
    Finalizer { finalize: table.finalize, value: slot_ptr(h, 1).cast() }
}

/// 破棄処理を呼ぶ。値を書いた物体の権限をまだ持っている (メモリは生きている) ときに呼ぶ。破棄処理は1回だけ呼ぶ。
/// 呼ぶ側は、物体を先に死んだことにしてから呼ぶ (破棄処理が panic しても2回呼ばないため)。
#[verifier::external_body]
#[inline]
pub(crate) fn run_finalizer(f: Finalizer, Tracked(perm): Tracked<&BlockPerm>) {
    unsafe { (f.finalize)(f.value) }
}

} // verus!

impl<T: Send + 'static> Table<T> {
    const VTABLE: VTable = VTable {
        type_id: TypeId::of::<T>,
        finalize: drop_value::<T>,
    };
}
