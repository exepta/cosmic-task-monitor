// SPDX-License-Identifier: MPL-2.0

use super::*;

impl AppModel {
    pub(super) fn steam_app_id_for_process(
        process: &sysinfo::Process,
        processes: &HashMap<Pid, sysinfo::Process>,
    ) -> Option<String> {
        if let Some(app_id) = Self::extract_steam_app_id_from_process(process) {
            return Some(app_id);
        }

        let mut visited = HashSet::new();
        let mut parent = process.parent();
        let mut depth = 0usize;

        while let Some(parent_pid) = parent {
            if depth >= 12 || !visited.insert(parent_pid) {
                break;
            }

            let Some(parent_process) = processes.get(&parent_pid) else {
                break;
            };

            if let Some(app_id) = Self::extract_steam_app_id_from_process(parent_process) {
                return Some(app_id);
            }

            parent = parent_process.parent();
            depth += 1;
        }

        None
    }

    pub(super) fn extract_steam_app_id_from_process(process: &sysinfo::Process) -> Option<String> {
        if let Some(app_id) = Self::extract_steam_app_id(process.name().to_string_lossy().as_ref())
        {
            return Some(app_id);
        }

        if let Some(cmd0) = process.cmd().first() {
            if let Some(app_id) = Self::extract_steam_app_id(cmd0.to_string_lossy().as_ref()) {
                return Some(app_id);
            }
        }

        if !process.cmd().is_empty() {
            let cmdline = process
                .cmd()
                .iter()
                .map(|part| part.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            if let Some(app_id) = Self::extract_steam_app_id(&cmdline) {
                return Some(app_id);
            }

            for arg in process.cmd() {
                if let Some(app_id) = Self::extract_steam_app_id(arg.to_string_lossy().as_ref()) {
                    return Some(app_id);
                }
            }
        }

        None
    }

    pub(super) fn extract_steam_app_id(value: &str) -> Option<String> {
        if value.trim().is_empty() {
            return None;
        }

        let lower = value.to_ascii_lowercase();
        for marker in ["appid=", "gameid=", "-gameid", "steam_app_", "rungameid/"] {
            if let Some(app_id) = Self::extract_decimal_after_marker(value, &lower, marker) {
                return Some(app_id);
            }
        }

        None
    }

    pub(super) fn extract_decimal_after_marker(
        original: &str,
        lower: &str,
        marker: &str,
    ) -> Option<String> {
        let mut offset = 0usize;
        while let Some(found) = lower[offset..].find(marker) {
            let start = offset + found + marker.len();
            if let Some(app_id) = Self::extract_decimal_from(original, start) {
                return Some(app_id);
            }
            offset = start;
        }
        None
    }

    pub(super) fn extract_decimal_from(value: &str, mut index: usize) -> Option<String> {
        let bytes = value.as_bytes();
        while index < bytes.len() {
            let c = bytes[index];
            if c.is_ascii_digit() {
                break;
            }
            if matches!(c, b' ' | b'=' | b':' | b'/' | b'-' | b'"' | b'\'') {
                index += 1;
                continue;
            }
            return None;
        }

        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }

        if start == index {
            return None;
        }

        let app_id = &value[start..index];
        if app_id == "0" {
            None
        } else {
            Some(app_id.to_string())
        }
    }

    pub(super) fn load_steam_app_meta(
        app_id: &str,
        default_icon: Option<icon::Handle>,
    ) -> SteamAppMeta {
        let name = Self::steam_manifest_name(app_id)
            .unwrap_or_else(|| crate::fl!("steam-app-fallback", app_id = app_id));
        let icon_handle = Self::steam_icon_path(app_id)
            .map(icon::from_path)
            .or(default_icon);

        SteamAppMeta { name, icon_handle }
    }

    pub(super) fn steam_manifest_name(app_id: &str) -> Option<String> {
        for library_root in Self::steam_library_roots() {
            let steamapps = Self::steamapps_dir(&library_root);
            let manifest = steamapps.join(format!("appmanifest_{app_id}.acf"));
            if !manifest.is_file() {
                continue;
            }

            if let Ok(content) = fs::read_to_string(&manifest) {
                if let Some(name) = Self::acf_value(&content, "name") {
                    let trimmed = name.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }

        None
    }

    pub(super) fn steam_install_dir(app_id: &str) -> Option<PathBuf> {
        for library_root in Self::steam_library_roots() {
            let steamapps = Self::steamapps_dir(&library_root);
            let manifest = steamapps.join(format!("appmanifest_{app_id}.acf"));
            if !manifest.is_file() {
                continue;
            }

            let Ok(content) = fs::read_to_string(&manifest) else {
                continue;
            };

            let Some(install_dir) = Self::acf_value(&content, "installdir") else {
                continue;
            };

            let path = steamapps.join("common").join(install_dir);
            if path.exists() {
                return Some(path);
            }
        }

        None
    }

    pub(super) fn steam_icon_path(app_id: &str) -> Option<PathBuf> {
        for steam_root in Self::steam_root_paths() {
            let app_dir = steam_root
                .join("appcache")
                .join("librarycache")
                .join(app_id);
            if !app_dir.is_dir() {
                continue;
            }

            if let Some(path) = Self::preferred_icon_path_in_dir(&app_dir) {
                return Some(path);
            }

            if let Ok(entries) = fs::read_dir(&app_dir) {
                let mut nested_dirs = entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir())
                    .collect::<Vec<_>>();
                nested_dirs.sort();

                for nested in nested_dirs {
                    if let Some(path) = Self::preferred_icon_path_in_dir(&nested) {
                        return Some(path);
                    }
                }
            }
        }

        None
    }

    pub(super) fn preferred_icon_path_in_dir(dir: &Path) -> Option<PathBuf> {
        for name in ["logo.png", "library_600x900.jpg", "library_header.jpg"] {
            let path = dir.join(name);
            if path.is_file() {
                return Some(path);
            }
        }

        let mut fallback = fs::read_dir(dir)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| {
                            matches!(
                                ext.to_ascii_lowercase().as_str(),
                                "png" | "jpg" | "jpeg" | "webp" | "svg"
                            )
                        })
                        .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        fallback.sort();
        fallback.into_iter().next()
    }

    pub(super) fn steam_root_paths() -> Vec<PathBuf> {
        let mut candidates = Vec::new();

        if let Ok(compat_root) = env::var("STEAM_COMPAT_CLIENT_INSTALL_PATH") {
            let path = PathBuf::from(compat_root);
            if path.is_dir() {
                candidates.push(path);
            }
        }

        if let Ok(home) = env::var("HOME") {
            let local_share = PathBuf::from(&home)
                .join(".local")
                .join("share")
                .join("Steam");
            if local_share.is_dir() {
                candidates.push(local_share);
            }

            let legacy = PathBuf::from(home).join(".steam").join("steam");
            if legacy.is_dir() {
                candidates.push(legacy);
            }
        }

        let mut seen = HashSet::new();
        let mut unique = Vec::new();
        for path in candidates {
            let key = path.to_string_lossy().to_string();
            if seen.insert(key) {
                unique.push(path);
            }
        }
        unique
    }

    pub(super) fn steam_library_roots() -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for steam_root in Self::steam_root_paths() {
            roots.push(steam_root.clone());
            let libraryfolders = steam_root.join("steamapps").join("libraryfolders.vdf");
            if let Ok(content) = fs::read_to_string(libraryfolders) {
                roots.extend(Self::steam_library_roots_from_vdf(&content));
            }
        }

        let mut seen = HashSet::new();
        let mut unique = Vec::new();
        for path in roots {
            if !path.is_dir() {
                continue;
            }
            let key = path.to_string_lossy().to_string();
            if seen.insert(key) {
                unique.push(path);
            }
        }
        unique
    }

    pub(super) fn steam_library_roots_from_vdf(vdf: &str) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        for line in vdf.lines() {
            let Some((key, value)) = Self::quoted_kv(line) else {
                continue;
            };
            if key != "path" {
                continue;
            }

            let unescaped = value.replace("\\\\", "\\");
            roots.push(PathBuf::from(unescaped));
        }
        roots
    }

    pub(super) fn steamapps_dir(root: &Path) -> PathBuf {
        if root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("steamapps"))
        {
            root.to_path_buf()
        } else {
            root.join("steamapps")
        }
    }

    pub(super) fn acf_value(content: &str, key: &str) -> Option<String> {
        for line in content.lines() {
            let Some((line_key, line_value)) = Self::quoted_kv(line) else {
                continue;
            };
            if line_key.eq_ignore_ascii_case(key) {
                return Some(line_value);
            }
        }
        None
    }

    pub(super) fn quoted_kv(line: &str) -> Option<(String, String)> {
        let mut parts = line.split('"');
        let _before_key = parts.next()?;
        let key = parts.next()?.trim();
        let _between = parts.next()?;
        let value = parts.next()?.trim();
        if key.is_empty() {
            return None;
        }
        Some((key.to_string(), value.to_string()))
    }
}
