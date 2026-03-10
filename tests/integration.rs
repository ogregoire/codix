use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn codix_cmd(dir: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codix"));
    cmd.current_dir(dir);
    cmd
}

fn setup_java_project(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/Foo.java"),
        r#"package com.foo;
public class Foo {
    public void bar() {}
}
"#,
    )
    .unwrap();
    fs::write(
        dir.join("src/Baz.java"),
        r#"package com.foo;
public class Baz extends Foo {
    private Foo helper;
    public void bar() { helper.bar(); }
}
"#,
    )
    .unwrap();
}

#[test]
fn test_init_creates_codix_dir() {
    let tmp = TempDir::new().unwrap();
    let out = codix_cmd(tmp.path()).arg("init").output().unwrap();
    assert!(out.status.success());
    assert!(tmp.path().join(".codix").exists());
    // Also creates index.db
    assert!(tmp.path().join(".codix/index.db").exists());
}

#[test]
fn test_init_indexes_files() {
    let tmp = TempDir::new().unwrap();
    setup_java_project(tmp.path());
    let out = codix_cmd(tmp.path()).arg("init").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Indexed 2 files"));
}

#[test]
fn test_no_init_error() {
    let tmp = TempDir::new().unwrap();
    let out = codix_cmd(tmp.path()).args(["find", "Foo"]).output().unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("codix init"));
}

#[test]
fn test_find_symbol() {
    let tmp = TempDir::new().unwrap();
    setup_java_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path()).args(["find", "Foo"]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Foo"));
    assert!(stdout.contains("class"));
}

#[test]
fn test_find_glob() {
    let tmp = TempDir::new().unwrap();
    setup_java_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path()).args(["find", "*oo"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Foo"));
}

#[test]
fn test_find_case_insensitive() {
    let tmp = TempDir::new().unwrap();
    setup_java_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["find", "foo", "-i"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Foo"));
}

#[test]
fn test_find_by_kind() {
    let tmp = TempDir::new().unwrap();
    setup_java_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["find", "*", "-k", "method"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("bar"));
    assert!(!stdout.contains("class"));
}

#[test]
fn test_symbols_in_file() {
    let tmp = TempDir::new().unwrap();
    setup_java_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["symbols", "src/Foo.java"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Foo"));
    assert!(stdout.contains("bar"));
}

#[test]
fn test_impls() {
    let tmp = TempDir::new().unwrap();
    setup_java_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["impls", "Foo"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Baz"));
}

#[test]
fn test_supers() {
    let tmp = TempDir::new().unwrap();
    setup_java_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["supers", "Baz"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Foo"));
}

#[test]
fn test_refs() {
    let tmp = TempDir::new().unwrap();
    setup_java_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["refs", "Foo"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    // Baz extends Foo and has a Foo field — both should show up
    assert!(stdout.contains("Baz"));
}

#[test]
fn test_package() {
    let tmp = TempDir::new().unwrap();
    setup_java_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["package", "com.foo"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Foo"));
    assert!(stdout.contains("Baz"));
}

#[test]
fn test_json_output() {
    let tmp = TempDir::new().unwrap();
    setup_java_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["find", "Foo", "-f", "json"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(parsed.is_array());
    assert!(!parsed.as_array().unwrap().is_empty());
}

#[test]
fn test_full_reindex() {
    let tmp = TempDir::new().unwrap();
    setup_java_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    // Full reindex
    let out = codix_cmd(tmp.path()).arg("index").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Indexed"));
}

#[test]
fn test_help_shows_all_commands() {
    let tmp = TempDir::new().unwrap();
    let out = codix_cmd(tmp.path()).arg("--help").output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("init"));
    assert!(stdout.contains("index"));
    assert!(stdout.contains("find"));
    assert!(stdout.contains("refs"));
    assert!(stdout.contains("impls"));
    assert!(stdout.contains("supers"));
    assert!(stdout.contains("callers"));
    assert!(stdout.contains("callees"));
    assert!(stdout.contains("symbols"));
    assert!(stdout.contains("package"));
}
