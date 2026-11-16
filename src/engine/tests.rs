use std::{
    fs,
    os::unix::net::UnixListener,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use super::prepare_socket;

static NEXT_RUNTIME_ID: AtomicU64 = AtomicU64::new(0);

struct TestRuntime(PathBuf);

impl TestRuntime {
    fn new() -> Self {
        let path = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!(
                "test-runtime-{}-{}",
                std::process::id(),
                NEXT_RUNTIME_ID.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn socket(&self) -> PathBuf {
        self.0.join("gammashift").join("gammashift.sock")
    }
}

impl Drop for TestRuntime {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn removes_a_refused_stale_socket() {
    let runtime = TestRuntime::new();
    let socket = runtime.socket();
    fs::create_dir_all(socket.parent().unwrap()).unwrap();
    drop(UnixListener::bind(&socket).unwrap());

    prepare_socket(&socket).await.unwrap();

    assert!(!socket.exists());
}

#[tokio::test]
async fn preserves_an_active_socket() {
    let runtime = TestRuntime::new();
    let socket = runtime.socket();
    fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();

    let error = prepare_socket(&socket).await.unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
    assert!(socket.exists());
    drop(listener);
}

#[tokio::test]
async fn refuses_to_replace_a_non_socket_file() {
    let runtime = TestRuntime::new();
    let socket = runtime.socket();
    fs::create_dir_all(socket.parent().unwrap()).unwrap();
    fs::write(&socket, b"preserve").unwrap();

    let error = prepare_socket(&socket).await.unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&socket).unwrap(), b"preserve");
}

#[test]
fn socket_path_helper_uses_the_supplied_runtime_directory() {
    let runtime = TestRuntime::new();

    assert_eq!(
        crate::socket_path_in(&runtime.0),
        expected_socket_path(&runtime.0)
    );
}

fn expected_socket_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("gammashift").join("gammashift.sock")
}
