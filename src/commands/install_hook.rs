use crate::git::find_repo_root;

const HOOK_MARKER: &str = "gradual check";
const HOOK_CONTENT: &str = "#!/bin/sh\n# Added by gradual. Do not edit this line.\ngradual check\n";

pub fn run() -> anyhow::Result<()> {
    let repo_root = find_repo_root()?;
    let hooks_dir = repo_root.join(".git/hooks");
    let hook_path = hooks_dir.join("pre-commit");

    if !hook_path.exists() {
        std::fs::create_dir_all(&hooks_dir)?;
        std::fs::write(&hook_path, HOOK_CONTENT)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o755))?;
        }
        println!("Installed pre-commit hook at {}", hook_path.display());
        return Ok(());
    }

    let existing = std::fs::read_to_string(&hook_path)?;
    if existing.contains(HOOK_MARKER) {
        println!("Pre-commit hook already installed at {}", hook_path.display());
        return Ok(());
    }

    eprintln!(
        "warning: {} already exists and does not contain 'gradual check'.",
        hook_path.display()
    );
    eprintln!(
        "To add gradual manually, append the following line to {}:",
        hook_path.display()
    );
    eprintln!();
    eprintln!("  gradual check");
    Ok(())
}
