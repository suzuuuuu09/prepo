use assert_cmd::Command;
use tempfile::tempdir;

#[test]
fn init_creates_a_valid_placeholder_configuration() {
    let directory = tempdir().expect("temporary directory");

    Command::cargo_bin("prepo")
        .expect("prepo binary")
        .current_dir(directory.path())
        .arg("init")
        .assert()
        .success();

    let configuration = std::fs::read_to_string(directory.path().join("prepo.toml"))
        .expect("generated configuration");

    assert!(configuration.contains("id = \"replace-me\""));
    assert!(configuration.contains("required = true") || !configuration.contains("required ="));
}
