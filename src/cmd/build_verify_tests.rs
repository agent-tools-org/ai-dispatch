// Cargo verify request tests for command semantics.
// Exports: no production API.
// Deps: parent build module.

use super::BuildRequest;
use std::path::Path;

#[test]
fn verify_request_keeps_cargo_test_semantics() {
    let request = BuildRequest::for_verify(
        Path::new("/tmp/project"),
        Some("cargo test --bin aid"),
    )
    .expect("cargo test is a supported verify command");
    assert_eq!(request.cargo_args(), ["test", "--message-format=json", "--bin", "aid"]);
}
