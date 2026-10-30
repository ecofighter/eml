//! rustls のクライアント接続を、I/O を持たない形で包む。
//!
//! eml のどの crate にも依存しない。バイトを汲む側 (`eml_vm` と `std/`) が I/O を受け持つ。

pub mod fixtures;
