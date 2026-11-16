use super::*;

#[test]
fn socket_path_is_scoped_to_the_supplied_runtime_directory() {
    assert_eq!(
        socket_path_in("/run/user/example"),
        std::path::PathBuf::from("/run/user/example/gammashift/gammashift.sock")
    );
}
