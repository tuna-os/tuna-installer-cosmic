//! Offline-install and sandbox plumbing.
//! Contract: ../../INSTALLER-FRONTENDS.md §3 (privileges) and §4 (offline).

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

pub fn in_flatpak() -> bool {
    Path::new("/.flatpak-info").exists()
}

/// Program + leading args that run fisherman with privileges.
///
/// Flatpak runtimes ship no pkexec; escalate host-side. The live ISO
/// symlinks the flatpak-bundled fisherman to /usr/local/bin and installs
/// the polkit policy for it (tunaOS customize-live.sh).
pub fn fisherman_command() -> Vec<String> {
    if in_flatpak() {
        vec![
            "flatpak-spawn".into(),
            "--host".into(),
            "pkexec".into(),
            "/usr/local/bin/fisherman".into(),
        ]
    } else {
        vec!["sudo".into(), "/usr/local/bin/fisherman".into()]
    }
}

/// Wrap argv so it executes on the host when sandboxed.
pub fn host_command(argv: &[&str]) -> Vec<String> {
    let mut cmd: Vec<String> = Vec::new();
    if in_flatpak() {
        cmd.push("flatpak-spawn".into());
        cmd.push("--host".into());
    }
    cmd.extend(argv.iter().map(|s| s.to_string()));
    cmd
}

fn run_host(argv: &[&str]) -> Option<String> {
    let cmd = host_command(argv);
    let out = Command::new(&cmd[0]).args(&cmd[1..]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Image ref of the booted live system, or None when not on live media.
/// Some(_) means the recipe may omit `image` (bootc installs the running container).
pub fn live_iso_image() -> Option<String> {
    let out = run_host(&["bootc", "status", "--json"])?;
    let status: serde_json::Value = serde_json::from_str(&out).ok()?;
    let img = status["status"]["booted"]["image"]["image"]["image"].as_str()?;
    if img.is_empty() {
        return None;
    }
    let live = Path::new("/run/ostree-live").exists()
        || std::fs::read_to_string("/proc/cmdline")
            .map(|c| c.contains("rd.live.image"))
            .unwrap_or(false);
    live.then(|| img.to_string())
}

/// Embedded OCI store roots present on this medium (§4B conventions).
pub fn offline_stores() -> Vec<String> {
    let mut stores: Vec<String> = Vec::new();
    if let Ok(env) = std::env::var("TUNA_OFFLINE_STORES") {
        stores.extend(env.split(':').map(str::to_string));
    }
    if let Ok(listing) = std::fs::read_to_string("/etc/tuna-installer/offline-stores") {
        stores.extend(
            listing
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_string),
        );
    }
    stores.push("/usr/share/tuna-installer/oci-store".into());
    let mut seen = HashSet::new();
    stores
        .into_iter()
        .filter(|s| seen.insert(s.clone()) && Path::new(s).is_dir())
        .collect()
}

/// Image refs available across the given stores.
pub fn offline_images(stores: &[String]) -> HashSet<String> {
    let mut refs = HashSet::new();
    for store in stores {
        let Some(out) = run_host(&["podman", "images", "--root", store, "--format", "json"])
        else {
            continue;
        };
        let Ok(imgs) = serde_json::from_str::<serde_json::Value>(&out) else {
            continue;
        };
        for img in imgs.as_array().into_iter().flatten() {
            for name in img["Names"].as_array().into_iter().flatten() {
                if let Some(n) = name.as_str() {
                    refs.insert(n.to_string());
                }
            }
        }
    }
    refs
}

/// Write the recipe 0600 in a fresh private directory (it may hold secrets).
///
/// Uses NamedTempFile (O_EXCL + O_NOFOLLOW + 0600) in a directory under
/// XDG_RUNTIME_DIR, falling back to the system temp dir. Never writes to a
/// fixed path: this used to be `<base>/tuna-installer/recipe.json` opened
/// with `create(true)`, which follows a pre-existing attacker symlink and
/// silently ignores the 0600 mode for a pre-existing file — a local user
/// could read the LUKS passphrase or swap the recipe root executes.
pub fn write_recipe(json: &str) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let mut f = NamedTempFile::new_in(base)?;
    f.write_all(json.as_bytes())?;
    // Keep the file after the TempFile is dropped; the caller removes it once
    // fisherman has exited. The unpredictable 0600 name survives here, so no
    // fixed path is ever handed to sudo/pkexec.
    let (_, path) = f.keep()?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_workdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tuna-installer-offline-{}-{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // ── sandbox detection / command construction ──────────────────────────────

    #[test]
    fn not_in_flatpak_in_test_environment() {
        // CI and developer machines are not sandboxed; the Flatpak branch of
        // fisherman_command/host_command is exercised by the live-ISO smoke.
        assert!(!in_flatpak());
    }

    #[test]
    fn fisherman_command_escalates_via_sudo_outside_flatpak() {
        assert_eq!(
            fisherman_command(),
            vec!["sudo".to_string(), "/usr/local/bin/fisherman".to_string()]
        );
    }

    #[test]
    fn host_command_passes_through_outside_flatpak() {
        assert_eq!(host_command(&["bootc", "status"]), vec!["bootc".to_string(), "status".to_string()]);
        assert_eq!(host_command(&[]), Vec::<String>::new());
    }

    // ── offline store discovery ───────────────────────────────────────────────

    #[test]
    fn offline_stores_parses_env_filters_non_dirs_and_dedups() {
        let dir = temp_workdir("stores");
        let a = dir.join("store-a");
        let b = dir.join("store-b");
        let not_a_dir = dir.join("plain-file");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(&not_a_dir, "x").unwrap();

        let value = format!("{}:{}:{}", a.display(), b.display(), not_a_dir.display());
        // SAFETY: single-threaded test; unique value; same var not used elsewhere.
        unsafe { std::env::set_var("TUNA_OFFLINE_STORES", &value) };
        let stores = offline_stores();
        unsafe { std::env::remove_var("TUNA_OFFLINE_STORES") };

        assert!(stores.contains(&a.display().to_string()), "{stores:?}");
        assert!(stores.contains(&b.display().to_string()), "{stores:?}");
        assert!(!stores.contains(&not_a_dir.display().to_string()), "{stores:?}");
        // The default store is not a directory in the test environment.
        assert!(!stores.iter().any(|s| s == "/usr/share/tuna-installer/oci-store"), "{stores:?}");

        let dup_value = format!("{}:{}", a.display(), a.display());
        unsafe { std::env::set_var("TUNA_OFFLINE_STORES", &dup_value) };
        let dedup = offline_stores();
        unsafe { std::env::remove_var("TUNA_OFFLINE_STORES") };
        assert_eq!(
            dedup.iter().filter(|s| s.as_str() == a.display().to_string()).count(),
            1,
            "{dedup:?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn offline_images_empty_without_stores() {
        assert!(offline_images(&[]).is_empty());
    }

    // ── recipe writing ────────────────────────────────────────────────────────

    #[test]
    fn write_recipe_creates_0600_unpredictable_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_workdir("recipe");
        // SAFETY: single-threaded test; XDG_RUNTIME_DIR is read immediately after.
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", &dir) };

        let path = write_recipe("{\"v\":1}").unwrap();
        assert!(path.starts_with(&dir));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"v\":1}");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "recipe must be 0600 (may hold secrets)");

        // Every write gets a fresh unpredictable path — never the fixed
        // `<base>/tuna-installer/recipe.json` that a local user could
        // pre-create, symlink, or read before chmod (gh-39).
        let path2 = write_recipe("{\"v\":2}").unwrap();
        assert_ne!(path, path2, "recipe path must not be a fixed constant");
        assert_eq!(std::fs::read_to_string(&path2).unwrap(), "{\"v\":2}");
        let mode = std::fs::metadata(&path2).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let name = path2.file_name().unwrap().to_string_lossy();
        assert!(
            !name.contains("recipe.json") || name.len() > 11,
            "file name must be unpredictable, got {name}"
        );

        unsafe { std::env::remove_var("XDG_RUNTIME_DIR") };
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_recipe_falls_back_to_tmp_without_runtime_dir() {
        use std::os::unix::fs::PermissionsExt;
        // SAFETY: single-threaded test.
        unsafe { std::env::remove_var("XDG_RUNTIME_DIR") };
        let path = write_recipe("{}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_file(&path);
    }
}
