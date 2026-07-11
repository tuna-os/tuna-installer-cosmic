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

/// Write the recipe 0600 under XDG_RUNTIME_DIR (it may hold secrets).
pub fn write_recipe(json: &str) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    let base = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let dir = Path::new(&base).join("tuna-installer");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    let path = dir.join("recipe.json");
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)?;
    f.write_all(json.as_bytes())?;
    Ok(path)
}
