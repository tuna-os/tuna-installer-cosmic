//! Fisherman recipe contract and installer domain data.
//!
//! Keeping these types outside the executable shell gives the serialized
//! backend interface one explicit owner shared by orchestration and UI code.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub disk: String,
    pub filesystem: String,
    #[serde(default)]
    pub btrfs_subvolumes: bool,
    pub encryption: Encryption,
    /// Empty in live-ISO mode: bootc installs the running container.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub image: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target_imgref: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bootloader: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub compose_fs_backend: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flatpaks: Vec<String>,
    /// Embedded OCI stores for offline installs (spec §4B).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_image_stores: Vec<String>,
    #[serde(rename = "distroID")]
    pub distro_id: String,
    #[serde(default = "default_selinux")]
    pub selinux_disabled: bool,
    pub hostname: String,
}

fn default_selinux() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Encryption {
    #[serde(rename = "type")]
    pub enc_type: String,
    #[serde(default)]
    pub passphrase: String,
}

impl Default for Encryption {
    fn default() -> Self {
        Self {
            enc_type: "none".into(),
            passphrase: String::new(),
        }
    }
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            disk: String::new(),
            filesystem: "xfs".into(),
            btrfs_subvolumes: false,
            encryption: Encryption::default(),
            image: "ghcr.io/tuna-os/albacore:gnome".into(),
            target_imgref: String::new(),
            bootloader: String::new(),
            compose_fs_backend: false,
            flatpaks: Vec::new(),
            additional_image_stores: Vec::new(),
            distro_id: "tunaos".into(),
            selinux_disabled: true,
            hostname: "tunaos".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub name: String,
    pub size: String,
    pub model: String,
    pub transport: String,
}

pub const FILESYSTEMS: [&str; 2] = ["xfs", "btrfs"];
