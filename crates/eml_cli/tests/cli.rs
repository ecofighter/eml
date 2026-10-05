use std::process::{Command, Output};

fn eml(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_eml"))
        .args(args)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/ui"))
        .output()
        .unwrap()
}

#[test]
fn check_succeeds_on_a_valid_file() {
    let output = eml(&["check", "run/basics/comments_only.em"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
}

#[test]
fn check_fails_with_diagnostics() {
    let output = eml(&["check", "check-fail/syntax/multiple_errors.em"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("[E0001]"));
}

#[test]
fn run_succeeds_on_a_valid_file() {
    let output = eml(&["run", "--debug-heap", "run/basics/comments_only.em"]);
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn run_does_not_execute_a_file_with_errors() {
    let output = eml(&["run", "check-fail/syntax/multiple_errors.em"]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn unreadable_file_is_a_usage_error() {
    let output = eml(&["check", "does/not/exist.em"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read `does/not/exist.em`"));
}

#[test]
fn missing_arguments_are_a_usage_error() {
    assert_eq!(eml(&[]).status.code(), Some(2));
    assert_eq!(eml(&["check"]).status.code(), Some(2));
    assert_eq!(eml(&["frobnicate"]).status.code(), Some(2));
}

#[test]
fn non_utf8_file_is_a_usage_error() {
    let path = std::env::temp_dir().join(format!("eml-cli-test-{}.em", std::process::id()));
    std::fs::write(&path, [0x66, 0x6e, 0xff, 0xfe]).unwrap();
    let output = eml(&["check", path.to_str().unwrap()]);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read"));
}

#[test]
fn run_fails_without_main_but_check_succeeds() {
    let path = std::env::temp_dir().join(format!("eml-cli-no-main-{}.em", std::process::id()));
    std::fs::write(&path, "f : Int -> Int\nf x = x\n").unwrap();
    let path_text = path.to_str().unwrap();
    assert_eq!(eml(&["check", path_text]).status.code(), Some(0));
    let output = eml(&["run", path_text]);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("E2003"));
}

#[test]
fn runtime_errors_exit_with_one() {
    let output = eml(&["run", "--debug-heap", "run-fail/basics/division_by_zero.em"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("runtime error: division by zero in `divide`")
    );
}

#[test]
fn run_prints_the_program_output() {
    let output = eml(&["run", "--debug-heap", "run/basics/hello.em"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello, world!\n");
}

#[test]
fn run_opens_an_absolute_path_as_is() {
    // UI テストは基準ディレクトリからの相対パスだけを使うので、絶対パスはここで確かめる (docs/spec/effects.md)
    let dir = std::env::temp_dir().join(format!("eml-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("input.txt");
    std::fs::write(&input, "from an absolute path").unwrap();
    let program = dir.join("absolute.em");
    let source = format!(
        "main : Unit -> <IO> Unit\nmain () =\n  let f = open \"{}\"\n  let (f, text) = read_all f\n  close f\n  println text\n",
        input.display()
    );
    std::fs::write(&program, source).unwrap();
    let output = eml(&["run", "--debug-heap", program.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "from an absolute path\n"
    );
}
