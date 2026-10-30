//! `tls` を外した `eml_io`。テストの driver は TLS の名前を知らない (docs/spec/vm.md の「時計と driver」)。

use std::io::ErrorKind;
use std::net::SocketAddr;

use eml_io::TestDriver;
use eml_rt::{Done, IoDriver, Op, Submit};

/// 受け付けで決まる操作を出し、その結果を返す。
fn run_op(driver: &mut TestDriver, key: u64, op: Op) -> std::io::Result<Done> {
    match driver.submit(key, op) {
        Submit::Done(result) => result,
        Submit::Pending => panic!("an op decided at submit is pending"),
    }
}

#[test]
fn the_tls_names_are_not_found() {
    let mut driver = TestDriver::new(None);
    let addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let Done::Listener { port, .. } = run_op(&mut driver, 1, Op::Listen { addr }).unwrap() else {
        panic!("a listen gives a listener")
    };
    for (key, host) in [(2, "tls.localhost"), (3, "tls-cut.localhost")] {
        let op = Op::Connect {
            host: host.to_owned(),
            port,
        };
        let err = run_op(&mut driver, key, op).expect_err("an unknown host");
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }
}
