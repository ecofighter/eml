//! `Bool`、`Ordering`、`IOErrorKind` のタグの定数が `eml_rt::externs` と `eml_lir` で同じであること。VM の `Bool` と `compare` の
//! 命令は、`eml_rt` の extern が返すタグを `eml_lir` の配置のタグとして読むので、これに頼る
//! (docs/spec/object-model.md の「語の ABI」)。

use eml_rt::externs;

#[test]
fn the_tag_constants_agree() {
    assert_eq!(externs::FALSE, i64::from(eml_lir::FALSE));
    assert_eq!(externs::TRUE, i64::from(eml_lir::TRUE));
    assert_eq!(externs::LT, i64::from(eml_lir::LT));
    assert_eq!(externs::EQ, i64::from(eml_lir::EQ));
    assert_eq!(externs::GT, i64::from(eml_lir::GT));
    assert_eq!(externs::IO_NOT_FOUND, i64::from(eml_lir::IO_NOT_FOUND));
    assert_eq!(
        externs::IO_PERMISSION_DENIED,
        i64::from(eml_lir::IO_PERMISSION_DENIED)
    );
    assert_eq!(
        externs::IO_NOT_AVAILABLE,
        i64::from(eml_lir::IO_NOT_AVAILABLE)
    );
    assert_eq!(externs::IO_NOT_UTF8, i64::from(eml_lir::IO_NOT_UTF8));
    assert_eq!(externs::IO_OTHER, i64::from(eml_lir::IO_OTHER));
}
