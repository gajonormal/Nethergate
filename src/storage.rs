//! Only Cargo's actual desktop output folders imply the development library.
use std::path::{Path, PathBuf};

pub fn development_root(exe: &Path) -> Option<PathBuf> {
    let profile = exe.parent()?;
    if !matches!(profile.file_name()?.to_str()?, "debug" | "release") {
        return None;
    }
    let target = profile.parent()?;
    if target.file_name()?.to_str()? != "target" {
        return None;
    }
    let desktop = target.parent()?;
    if desktop.file_name()?.to_str()? != "src-tauri" {
        return None;
    }
    let root = desktop.parent()?;
    root.join("src/hub.rs")
        .is_file()
        .then(|| root.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_copies_inside_project_never_select_development_worlds() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(
            development_root(&root.join("src-tauri/target/debug/nethergate-desktop.exe")),
            Some(root.to_path_buf())
        );
        assert_eq!(
            development_root(&root.join("src-tauri/target/release/nethergate-desktop.exe")),
            Some(root.to_path_buf())
        );
        assert!(development_root(
            &root.join("hub-data/installer-validation/install/nethergate-desktop.exe")
        )
        .is_none());
        assert!(development_root(&root.join("Downloads/nethergate-desktop.exe")).is_none());
    }
}
