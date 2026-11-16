pub mod cli;
pub mod engine;
pub mod message;
pub mod sender;

mod gamma_relay_interface;

pub const DEFAULT_TEMPERATURE: f64 = 6500.0;
pub const DEFAULT_BRIGHTNESS: f64 = 1.0;
pub const DEFAULT_GAMMA: f64 = 1.0;

pub fn socket_path() -> std::io::Result<std::path::PathBuf> {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "XDG_RUNTIME_DIR is not set; cannot locate the gammashift socket",
            )
        })?;
    Ok(socket_path_in(runtime_dir))
}

fn socket_path_in(runtime_dir: impl AsRef<std::path::Path>) -> std::path::PathBuf {
    runtime_dir
        .as_ref()
        .join("gammashift")
        .join("gammashift.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_is_scoped_to_the_supplied_runtime_directory() {
        assert_eq!(
            socket_path_in("/run/user/example"),
            std::path::PathBuf::from("/run/user/example/gammashift/gammashift.sock")
        );
    }
}
