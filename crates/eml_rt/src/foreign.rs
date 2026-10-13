//! `Foreign` の物体 (docs/spec/object-model.md の「種類」)。ペイロードの先頭に破棄
//! 処理の表への参照を置き、その後に Rust の値を置く。

use std::any::TypeId;
use std::marker::PhantomData;
use std::ptr::NonNull;

use crate::meta::{self, Header};

pub(crate) struct VTable {
    /// 型の照らし合わせに使う。表の番地は型ごとに1つと限らないので、番地では比べない。
    type_id: fn() -> TypeId,
    finalize: unsafe fn(*mut u8),
}

struct Table<T>(PhantomData<T>);

impl<T: Send + 'static> Table<T> {
    const VTABLE: VTable = VTable {
        type_id: TypeId::of::<T>,
        finalize: drop_value::<T>,
    };
}

unsafe fn drop_value<T>(value: *mut u8) {
    unsafe { value.cast::<T>().drop_in_place() }
}

/// # Safety
/// `h` は種類 `Foreign` の初期化したヘッダを指し、ペイロードは表の1語と `T` を入れられる。
pub(crate) unsafe fn write<T: Send + 'static>(h: NonNull<Header>, value: T) {
    let table: &'static VTable = &Table::<T>::VTABLE;
    unsafe {
        meta::slot(h, 0).cast::<&'static VTable>().write(table);
        meta::slot(h, 1).cast::<T>().write(value);
    }
}

/// 値が `T` なら、その位置を返す。
///
/// # Safety
/// `h` は生きている `Foreign` の物体を指す。
pub(crate) unsafe fn value<T: 'static>(h: NonNull<Header>) -> Option<*mut T> {
    let table = unsafe { meta::slot(h, 0).cast::<&'static VTable>().read() };
    ((table.type_id)() == TypeId::of::<T>()).then(|| unsafe { meta::slot(h, 1) }.cast())
}

/// 破棄処理と値の位置。物体の状態を替えたあとで呼べるように、先に読み出しておく。
pub(crate) struct Finalizer {
    finalize: unsafe fn(*mut u8),
    value: *mut u8,
}

impl Finalizer {
    /// # Safety
    /// 値はまだ破棄していず、位置のメモリは生きている。
    pub(crate) unsafe fn run(self) {
        unsafe { (self.finalize)(self.value) }
    }
}

/// # Safety
/// `h` は生きている `Foreign` の物体を指し、破棄処理をまだ呼んでいない。
pub(crate) unsafe fn finalizer(h: NonNull<Header>) -> Finalizer {
    unsafe {
        let table = meta::slot(h, 0).cast::<&'static VTable>().read();
        Finalizer {
            finalize: table.finalize,
            value: meta::slot(h, 1).cast(),
        }
    }
}
