//! Starting at login, with a launch agent: a small file in `~/Library/LaunchAgents` that asks
//! macOS to run this program when the user logs in. Turning it off deletes the file.

use std::path::PathBuf;

const LABEL: &str = "dev.iceout";

fn plist_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
}

/// Whether the program starts at login.
pub fn enabled() -> bool {
    plist_path().is_some_and(|path| path.exists())
}

/// Turns starting at login on or off. Returns whether it worked.
pub fn set(on: bool) -> bool {
    let Some(path) = plist_path() else { return false };
    if !on {
        return std::fs::remove_file(&path).is_ok() || !path.exists();
    }
    let Ok(program) = std::env::current_exe() else { return false };
    let text = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{LABEL}</string>
    <key>ProgramArguments</key><array><string>{}</string></array>
    <key>RunAtLoad</key><true/>
    <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#,
        program.display()
    );
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).ok();
    }
    std::fs::write(path, text).is_ok()
}
