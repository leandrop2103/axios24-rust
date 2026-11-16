use std::path::PathBuf;
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub const GITHUB_REPO: &str = "leandrop2103/axios24-rust";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct ReleaseUpdateInfo {
    pub tag: String,
    pub version: String,
    pub notes: String,
    pub download_url: String,
}

fn new_silent_cmd(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Cleans up leftover `.old` executable from a previous hot-update
pub fn cleanup_old_exe() {
    if let Ok(cur) = std::env::current_exe() {
        let old = old_exe_path(&cur);
        if old.exists() {
            let _ = std::fs::remove_file(&old);
        }
        let tmp = cur.with_file_name("axios24_update.tmp");
        if tmp.exists() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

fn old_exe_path(cur: &std::path::Path) -> PathBuf {
    let mut name = cur
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "axios24".to_string());
    name.push_str(".old");
    cur.with_file_name(name)
}

pub fn parse_semver(v: &str) -> (u32, u32, u32) {
    let clean = v.trim().trim_start_matches('v').trim_start_matches('V');
    let mut parts = clean.split(|c: char| !c.is_ascii_digit());
    let maj = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let min = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let pat = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (maj, min, pat)
}

pub fn is_newer_version(remote: &str, current: &str) -> bool {
    parse_semver(remote) > parse_semver(current)
}

fn extract_json_str(src: &str, key: &str) -> Option<String> {
    let pat = format!("\"{}\":", key);
    let idx = src.find(&pat)?;
    let rest = src[idx + pat.len()..].trim_start();
    if !rest.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut esc = false;
    for ch in rest[1..].chars() {
        if esc {
            match ch {
                'n' => out.push('\n'),
                'r' => {}
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                other => out.push(other),
            }
            esc = false;
        } else if ch == '\\' {
            esc = true;
        } else if ch == '"' {
            return Some(out);
        } else {
            out.push(ch);
        }
    }
    None
}

fn extract_download_url(src: &str) -> Option<String> {
    let mut urls = Vec::new();
    let mut pos = 0;
    let pat = "\"browser_download_url\":";
    while let Some(found) = src[pos..].find(pat) {
        let start = pos + found + pat.len();
        if let Some(url) = extract_json_str(&src[pos + found..], "browser_download_url") {
            urls.push(url);
        }
        pos = start;
    }
    if cfg!(windows) {
        urls.iter()
            .find(|u| u.to_lowercase().ends_with(".exe"))
            .cloned()
            .or_else(|| urls.into_iter().next())
    } else {
        urls.iter()
            .find(|u| !u.to_lowercase().ends_with(".exe"))
            .cloned()
            .or_else(|| urls.into_iter().next())
    }
}

pub fn parse_github_release_json(json: &str) -> Option<ReleaseUpdateInfo> {
    let tag = extract_json_str(json, "tag_name")?;
    let version = tag.trim().trim_start_matches('v').trim_start_matches('V').to_string();
    let notes = extract_json_str(json, "body").unwrap_or_else(|| "Melhorias e correções.".to_string());
    let download_url = extract_download_url(json).unwrap_or_default();
    Some(ReleaseUpdateInfo {
        tag,
        version,
        notes,
        download_url,
    })
}

/// Checks GitHub Releases API for a newer version
pub fn fetch_latest_release() -> Result<Option<ReleaseUpdateInfo>, String> {
    let api_url = format!("https://api.github.com/repos/{}/releases/latest", GITHUB_REPO);
    let output = new_silent_cmd("curl")
        .args([
            "-sL",
            "-m",
            "6",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: axios24-rust-updater",
            &api_url,
        ])
        .output()
        .map_err(|e| format!("Falha ao executar curl: {}", e))?;

    if !output.status.success() {
        return Err("Não foi possível consultar o GitHub.".to_string());
    }
    let body = String::from_utf8_lossy(&output.stdout);
    if let Some(info) = parse_github_release_json(&body) {
        if is_newer_version(&info.version, CURRENT_VERSION) {
            Ok(Some(info))
        } else {
            Ok(None)
        }
    } else {
        Ok(None)
    }
}

/// Downloads the release asset and hot-swaps the running executable
pub fn download_and_hot_update(download_url: &str) -> Result<(), String> {
    if download_url.is_empty() {
        return Err("URL de download não encontrada na release do GitHub.".to_string());
    }
    let cur_exe = std::env::current_exe().map_err(|e| format!("Erro current_exe: {}", e))?;
    let tmp_exe = cur_exe.with_file_name("axios24_update.tmp");
    let old_exe = old_exe_path(&cur_exe);

    let status = new_silent_cmd("curl")
        .args(["-fSL", "-m", "60", "-o"])
        .arg(&tmp_exe)
        .arg(download_url)
        .status()
        .map_err(|e| format!("Erro ao baixar atualização: {}", e))?;

    if !status.success() {
        let _ = std::fs::remove_file(&tmp_exe);
        return Err("Falha no download da atualização no GitHub.".to_string());
    }

    let meta = std::fs::metadata(&tmp_exe).map_err(|e| format!("Arquivo inválido: {}", e))?;
    if meta.len() < 100_000 {
        let _ = std::fs::remove_file(&tmp_exe);
        return Err("Arquivo baixado está incompleto ou corrompido.".to_string());
    }

    let head = std::fs::read(&tmp_exe)
        .map(|b| b.get(..4).unwrap_or(&[]).to_vec())
        .unwrap_or_default();
    #[cfg(windows)]
    if head.get(..2) != Some(b"MZ") {
        let _ = std::fs::remove_file(&tmp_exe);
        return Err("O arquivo baixado não é um executável Windows válido.".to_string());
    }
    #[cfg(target_os = "linux")]
    if head.get(..4) != Some(b"\x7fELF") {
        let _ = std::fs::remove_file(&tmp_exe);
        return Err("O arquivo baixado não é um binário Linux ELF válido.".to_string());
    }

    if old_exe.exists() {
        let _ = std::fs::remove_file(&old_exe);
    }
    std::fs::rename(&cur_exe, &old_exe)
        .map_err(|e| format!("Não foi possível substituir executável atual: {}", e))?;

    if let Err(e) = std::fs::rename(&tmp_exe, &cur_exe) {
        let _ = std::fs::rename(&old_exe, &cur_exe);
        return Err(format!("Falha ao aplicar nova versão: {}", e));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&cur_exe, std::fs::Permissions::from_mode(0o755));
    }

    Command::new(&cur_exe)
        .spawn()
        .map_err(|e| format!("Atualizado, mas falhou ao reiniciar: {}", e))?;
    std::process::exit(0);
}
