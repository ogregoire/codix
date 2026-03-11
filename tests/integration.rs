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
    assert!(stdout.contains("Indexed 2 Java files"));
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

fn setup_multi_package_project(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("src/foo")).unwrap();
    fs::write(
        dir.join("src/foo/Repository.java"),
        r#"package com.foo;
public interface Repository {
    void save(Object o);
}
"#,
    ).unwrap();
    fs::write(
        dir.join("src/foo/Person.java"),
        r#"package com.foo;
public class Person {}
"#,
    ).unwrap();

    fs::create_dir_all(dir.join("src/bar")).unwrap();
    fs::write(
        dir.join("src/bar/UserService.java"),
        r#"package com.bar;
import com.foo.Repository;
import com.foo.Person;
public class UserService implements Repository {
    private Person person;
    public void save(Object o) {}
}
"#,
    ).unwrap();

    fs::create_dir_all(dir.join("src/baz")).unwrap();
    fs::write(
        dir.join("src/baz/Client.java"),
        r#"package com.baz;
import com.foo.*;
public class Client extends Person {}
"#,
    ).unwrap();
}

#[test]
fn test_cross_package_import_resolution() {
    let tmp = TempDir::new().unwrap();
    setup_multi_package_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path()).args(["impls", "com.foo.Repository"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("UserService"));
}

#[test]
fn test_wildcard_import_resolution() {
    let tmp = TempDir::new().unwrap();
    setup_multi_package_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path()).args(["supers", "Client"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Person"));
}

#[test]
fn test_same_package_implicit_resolution() {
    let tmp = TempDir::new().unwrap();
    setup_multi_package_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path()).args(["refs", "com.foo.Person"]).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("UserService"), "UserService should reference Person via field type");
    assert!(stdout.contains("Client"), "Client should reference Person via extends");
}

fn setup_method_call_project(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("src/foo")).unwrap();
    fs::write(
        dir.join("src/foo/Repository.java"),
        r#"package com.foo;
public interface Repository {
    void save(Object o);
    Object findById(int id);
}
"#,
    ).unwrap();

    fs::create_dir_all(dir.join("src/bar")).unwrap();
    fs::write(
        dir.join("src/bar/Service.java"),
        r#"package com.bar;
import com.foo.Repository;
public class Service {
    private Repository repo;
    public void doWork() {
        repo.save(null);
    }
}
"#,
    ).unwrap();
}

#[test]
fn test_callers_with_receiver_resolution() {
    let tmp = TempDir::new().unwrap();
    setup_method_call_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["callers", "com.foo.Repository.save*"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("doWork"), "doWork should be a caller of Repository.save");
}

#[test]
fn test_callees_with_receiver_resolution() {
    let tmp = TempDir::new().unwrap();
    setup_method_call_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["callees", "com.bar.Service.doWork*"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("save"), "save should be a callee of doWork");
}

#[test]
fn test_inner_class_symbols() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("src/Outer.java"),
        r#"package com.foo;
public class Outer {
    public static class Inner {
        public void doWork() {}
    }
    public interface Callback {
        void onComplete();
    }
}
"#,
    ).unwrap();
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["symbols", "src/Outer.java"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Outer"), "should contain Outer class");
    assert!(stdout.contains("Inner"), "should contain Inner class");
    assert!(stdout.contains("Callback"), "should contain Callback interface");
    assert!(stdout.contains("doWork"), "should contain Inner's method");
    assert!(stdout.contains("onComplete"), "should contain Callback's method");
}

#[test]
fn test_annotation_refs() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("src/MyAnnotation.java"),
        r#"package com.foo;
public @interface MyAnnotation {}
"#,
    ).unwrap();
    fs::write(
        tmp.path().join("src/Service.java"),
        r#"package com.foo;
public class Service {
    @MyAnnotation
    public void save() {}
}
"#,
    ).unwrap();
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["refs", "com.foo.MyAnnotation"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("save"), "save should reference MyAnnotation via @MyAnnotation");
}

#[test]
fn test_record_compact_constructor() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("src/Point.java"),
        r#"package com.foo;
public record Point(int x, int y) {
    Point {
        if (x < 0) throw new IllegalArgumentException();
    }
}
"#,
    ).unwrap();
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["symbols", "src/Point.java"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("record"), "should contain Point record");
    assert!(stdout.contains("constructor"), "should contain compact constructor");
    assert!(stdout.contains("Point(int,int)"), "compact constructor should have record's parameters");
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

fn setup_js_project(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/app.js"),
        r#"class App {
    #name;

    constructor(name) {
        this.#name = name;
    }

    run() {
        console.log(this.#name);
    }
}

function main() {
    const app = new App("codix");
    app.run();
}

const helper = () => {
    return 42;
};
"#,
    ).unwrap();
    fs::write(
        dir.join("src/service.ts"),
        r#"class Service extends App {
    serve(): void {
        this.run();
    }
}
"#,
    ).unwrap();
}

#[test]
fn test_js_class_extraction() {
    let tmp = TempDir::new().unwrap();
    setup_js_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["symbols", "src/app.js"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("App"), "should contain App class");
    assert!(stdout.contains("constructor"), "should contain constructor");
    assert!(stdout.contains("run"), "should contain run method");
    assert!(stdout.contains("main"), "should contain main function");
    assert!(stdout.contains("helper"), "should contain helper function");
    assert!(stdout.contains("function"), "should show function kind");
}

#[test]
fn test_js_extends() {
    let tmp = TempDir::new().unwrap();
    setup_js_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["impls", "App"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Service"), "Service extends App");
}

#[test]
fn test_js_init_indexes_js_files() {
    let tmp = TempDir::new().unwrap();
    setup_js_project(tmp.path());
    let out = codix_cmd(tmp.path()).arg("init").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Indexed 2 JavaScript files"), "stdout was: {}", stdout);
}

fn setup_go_project(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/app.go"),
        r#"package main

type Repository interface {
	Save(item Item)
}

type Item struct {
	Name string
}

type Service struct {
	repo Repository
}

func (s *Service) Process() {
	s.repo.Save(Item{})
}

func NewService(r Repository) *Service {
	return &Service{repo: r}
}
"#,
    ).unwrap();
}

#[test]
fn test_go_struct_and_interface() {
    let tmp = TempDir::new().unwrap();
    setup_go_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["symbols", "src/app.go"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Repository"), "should contain Repository interface");
    assert!(stdout.contains("interface"), "should show interface kind");
    assert!(stdout.contains("Item"), "should contain Item struct");
    assert!(stdout.contains("struct"), "should show struct kind");
    assert!(stdout.contains("Service"), "should contain Service struct");
    assert!(stdout.contains("Process"), "should contain Process method");
    assert!(stdout.contains("NewService"), "should contain NewService function");
}

#[test]
fn test_go_field_type_refs() {
    let tmp = TempDir::new().unwrap();
    setup_go_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["refs", "main.Repository"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("repo"), "Service.repo should reference Repository via field type");
}

#[test]
fn test_go_init_indexes_files() {
    let tmp = TempDir::new().unwrap();
    setup_go_project(tmp.path());
    let out = codix_cmd(tmp.path()).arg("init").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Indexed 1 Go file"), "stdout was: {}", stdout);
}

fn setup_rust_project(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/app.rs"),
        r#"pub trait Repository {
    fn save(&self, item: Item);
    fn find(&self, id: u32) -> Item;
}

pub struct Item {
    pub name: String,
}

pub struct Service {
    repo: Box<dyn Repository>,
}

impl Service {
    pub fn new(repo: Box<dyn Repository>) -> Self {
        Service { repo }
    }

    pub fn process(&self) {
        let item = self.repo.find(1);
        self.repo.save(item);
    }
}

pub enum Status {
    Active,
    Inactive { reason: String },
}

pub type AppResult = Result<String, String>;
"#,
    ).unwrap();
}

#[test]
fn test_rust_symbols() {
    let tmp = TempDir::new().unwrap();
    setup_rust_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["symbols", "src/app.rs"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Repository"), "should contain Repository trait");
    assert!(stdout.contains("trait"), "should show trait kind");
    assert!(stdout.contains("Item"), "should contain Item struct");
    assert!(stdout.contains("Service"), "should contain Service struct");
    assert!(stdout.contains("process"), "should contain process method");
    assert!(stdout.contains("Status"), "should contain Status enum");
    assert!(stdout.contains("Active"), "should contain Active variant");
    assert!(stdout.contains("Inactive"), "should contain Inactive variant");
    assert!(stdout.contains("AppResult"), "should contain AppResult type alias");
    assert!(stdout.contains("type-alias"), "should show type-alias kind");
}

#[test]
fn test_rust_field_type_refs() {
    let tmp = TempDir::new().unwrap();
    setup_rust_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["refs", "Repository"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("repo"), "Service.repo should reference Repository via field type");
}

#[test]
fn test_rust_init_indexes_files() {
    let tmp = TempDir::new().unwrap();
    setup_rust_project(tmp.path());
    let out = codix_cmd(tmp.path()).arg("init").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Indexed 1 Rust file"), "stdout was: {}", stdout);
}

fn setup_python_project(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("src/app.py"),
        r#"from typing import List


class Repository:
    def save(self, item: "Item") -> None:
        pass

    def find(self, id: int) -> "Item":
        pass


class Item:
    name: str
    id: int


class Service:
    repo: Repository

    def __init__(self, repo: Repository):
        self.repo = repo

    def process(self) -> None:
        item = self.repo.find(1)
        self.repo.save(item)


@dataclass
class Config:
    debug: bool
"#,
    ).unwrap();
}

#[test]
fn test_python_symbols() {
    let tmp = TempDir::new().unwrap();
    setup_python_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["symbols", "src/app.py"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Repository"), "should contain Repository class");
    assert!(stdout.contains("class"), "should show class kind");
    assert!(stdout.contains("Item"), "should contain Item class");
    assert!(stdout.contains("Service"), "should contain Service class");
    assert!(stdout.contains("__init__"), "should contain constructor");
    assert!(stdout.contains("constructor"), "should show constructor kind");
    assert!(stdout.contains("process"), "should contain process method");
    assert!(stdout.contains("Config"), "should contain Config class");
}

#[test]
fn test_python_field_type_refs() {
    let tmp = TempDir::new().unwrap();
    setup_python_project(tmp.path());
    codix_cmd(tmp.path()).arg("init").output().unwrap();

    let out = codix_cmd(tmp.path())
        .args(["refs", "Repository"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("repo"), "Service.repo should reference Repository via field type");
}

#[test]
fn test_python_init_indexes_files() {
    let tmp = TempDir::new().unwrap();
    setup_python_project(tmp.path());
    let out = codix_cmd(tmp.path()).arg("init").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Indexed 1 Python file"), "stdout was: {}", stdout);
}
