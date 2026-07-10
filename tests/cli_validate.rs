use assert_cmd::Command;
use tempfile::tempdir;

#[test]
fn validate_accepts_a_minimal_required_path_check() {
    let directory = tempdir().expect("temporary directory");
    std::fs::write(
        directory.path().join("prepo.toml"),
        r#"
version = 1

[project]
name = "example"

[[checks]]
id = "repository-marker"
type = "path"
name = "Repository marker"
path = ".git"
expected = "exists"
kind = "any"
"#,
    )
    .expect("configuration");

    Command::cargo_bin("prepo")
        .expect("prepo binary")
        .current_dir(directory.path())
        .arg("validate")
        .assert()
        .success()
        .stdout("Configuration is valid.\n")
        .stderr("");
}
