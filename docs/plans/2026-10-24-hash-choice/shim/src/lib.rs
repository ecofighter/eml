//! `rustc-hash` の `FxHashMap`、`FxHashSet`、`FxBuildHasher` の名前で、別のハッシュ関数を使わせる。どれを使うかは
//! `--cfg eml_hash_<名前>` で選ぶ。どれも種を固定し、FxHash と同じく実行ごとに順が変わらないようにする。
#![allow(unexpected_cfgs)]

use std::hash::BuildHasher;

pub type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuildHasher>;
pub type FxHashSet<T> = std::collections::HashSet<T, FxBuildHasher>;

#[derive(Clone, Copy, Default, Debug)]
pub struct FxBuildHasher;

#[cfg(eml_hash_ahash)]
impl BuildHasher for FxBuildHasher {
    type Hasher = ahash::AHasher;

    #[inline(always)]
    fn build_hasher(&self) -> ahash::AHasher {
        static STATE: ahash::RandomState = ahash::RandomState::with_seeds(1, 2, 3, 4);
        STATE.build_hasher()
    }
}

#[cfg(eml_hash_fold)]
impl BuildHasher for FxBuildHasher {
    type Hasher = foldhash::fast::FoldHasher<'static>;

    #[inline(always)]
    fn build_hasher(&self) -> Self::Hasher {
        foldhash::fast::FixedState::default().build_hasher()
    }
}

#[cfg(eml_hash_rapid)]
impl BuildHasher for FxBuildHasher {
    type Hasher = rapidhash::fast::RapidHasher<'static>;

    #[inline(always)]
    fn build_hasher(&self) -> Self::Hasher {
        Default::default()
    }
}
