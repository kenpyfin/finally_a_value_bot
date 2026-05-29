//! Built-in hooks live in the repository `builtin_hooks/` tree.

use crate::config::Config;
use std::path::PathBuf;

/// Resolve the on-disk `builtin_hooks/` directory.
///
/// Precedence:
/// 1. `FINALLY_A_VALUE_BOT_BUILTIN_HOOKS` if set and path exists
/// 2. Parent of workspace root + `builtin_hooks`
/// 3. Current working directory + `builtin_hooks`
/// 4. Parent of current executable + `builtin_hooks`
/// 5. Compile-time `CARGO_MANIFEST_DIR/builtin_hooks`
pub fn resolve_builtin_hooks_dir(config: &Config) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("FINALLY_A_VALUE_BOT_BUILTIN_HOOKS") {
        let pb = PathBuf::from(p.trim());
        if pb.is_dir() {
            return Some(pb);
        }
    }

    let parent_builtin = config
        .workspace_root_absolute()
        .parent()
        .map(|p| p.join("builtin_hooks"));
    if let Some(ref p) = parent_builtin {
        if p.is_dir() {
            return Some(p.clone());
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let p = cwd.join("builtin_hooks");
        if p.is_dir() {
            return Some(p);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let p = parent.join("builtin_hooks");
            if p.is_dir() {
                return Some(p);
            }
        }
    }

    let manifest_builtin = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("builtin_hooks");
    if manifest_builtin.is_dir() {
        return Some(manifest_builtin);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_config;
    use std::fs;

    #[test]
    fn resolve_finds_sibling_of_workspace() {
        let tmp =
            std::env::temp_dir().join(format!("fab_builtin_hooks_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(tmp.join("workspace")).expect("workspace dir");
        fs::create_dir_all(tmp.join("builtin_hooks")).expect("builtin hooks dir");

        let mut config = test_config();
        config.workspace_dir = tmp.join("workspace").to_string_lossy().to_string();

        let got = resolve_builtin_hooks_dir(&config).expect("expected builtin_hooks");
        assert_eq!(got, tmp.join("builtin_hooks"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
