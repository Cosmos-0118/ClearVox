//! Persistent settings in `%APPDATA%\Sordino\settings.json`, plus the launch-at-login Run key.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    /// Endpoint ID of the physical microphone ("" = system default).
    pub input_id: String,
    /// Endpoint ID of the virtual cable's playback side.
    pub output_id: String,
    pub denoise: bool,
    pub gate: bool,
    pub highpass: bool,
    pub limiter: bool,
    pub agc: bool,
    /// Denoiser: "dpdfnet8" (best, default), "dpdfnet2" (lighter) or "rnnoise" (minimal).
    pub engine: String,
    pub use_as_default: bool,
    /// Default capture device before Sordino took over, so it can be restored.
    pub previous_default_input_id: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            input_id: String::new(),
            output_id: String::new(),
            denoise: true,
            gate: true,
            highpass: true,
            limiter: true,
            agc: true,
            engine: "dpdfnet8".into(),
            use_as_default: true,
            previous_default_input_id: String::new(),
        }
    }
}

fn dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Sordino")
}

fn path() -> PathBuf {
    dir().join("settings.json")
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(dir());
        if let Ok(json) = serde_json::to_string_pretty(self) {
            // Write-then-rename so a crash can't leave a half-written file.
            let tmp = path().with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, path());
            }
        }
    }
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Sordino";

pub fn launch_at_login() -> bool {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|k| k.get_value::<String, _>(RUN_VALUE))
        .is_ok()
}

pub fn set_launch_at_login(on: bool) -> std::io::Result<()> {
    use winreg::{enums::{HKEY_CURRENT_USER, KEY_SET_VALUE}, RegKey};
    let key = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)?;
    if on {
        let exe = std::env::current_exe()?;
        key.set_value(RUN_VALUE, &format!("\"{}\"", exe.display()))
    } else {
        match key.delete_value(RUN_VALUE) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }
}
