use crate::{cdp, paths};
use anyhow::{Context, Result, bail};
use reqwest::{Url, blocking::Client};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use sysinfo::{ProcessesToUpdate, System};
use zip::ZipArchive;

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/sundy-li/CodeFace/releases/latest";
const MAX_UPDATE_SIZE: usize = 150 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Version(u64, u64, u64);

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct LatestRelease {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum UpdateOutcome {
    UpToDate,
    Restarting,
}

fn parse_version(value: &str) -> Result<Version> {
    let value = value.trim().strip_prefix('v').unwrap_or(value.trim());
    let mut parts = value.split('.');
    let version = Version(
        parts
            .next()
            .context("version is missing its major number")?
            .parse()?,
        parts
            .next()
            .context("version is missing its minor number")?
            .parse()?,
        parts
            .next()
            .context("version is missing its patch number")?
            .parse()?,
    );
    if parts.next().is_some() {
        bail!("version must use major.minor.patch format");
    }
    Ok(version)
}

fn platform_asset_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "CodeFace-macOS.zip"
    } else {
        "CodeFace-Windows.zip"
    }
}

fn trusted_github_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).context("update asset URL is invalid")?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || !(host == "github.com"
            || host == "api.github.com"
            || host.ends_with(".githubusercontent.com"))
    {
        bail!("update asset must use a trusted GitHub HTTPS URL");
    }
    Ok(url)
}

fn download(client: &Client, url: &str, limit: usize) -> Result<Vec<u8>> {
    let url = trusted_github_url(url)?;
    let response = client
        .get(url)
        .send()
        .context("failed to download CodeFace update")?
        .error_for_status()
        .context("CodeFace update download failed")?;
    trusted_github_url(response.url().as_str())?;
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        bail!("CodeFace update exceeds the size limit");
    }
    let mut bytes = Vec::new();
    response.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        bail!("CodeFace update exceeds the size limit");
    }
    Ok(bytes)
}

fn expected_checksum(text: &[u8]) -> Result<String> {
    let text = std::str::from_utf8(text).context("update checksum is not UTF-8")?;
    let checksum = text.split_whitespace().next().unwrap_or_default();
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("update checksum file is invalid");
    }
    Ok(checksum.to_ascii_lowercase())
}

fn verify_checksum(bytes: &[u8], expected: &str) -> Result<()> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != expected {
        bail!("downloaded CodeFace update failed SHA-256 verification");
    }
    Ok(())
}

fn extract_zip(bytes: &[u8], target: &Path) -> Result<()> {
    if target.exists() {
        fs::remove_dir_all(target)?;
    }
    fs::create_dir_all(target)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).context("update ZIP is invalid")?;
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let relative = entry
            .enclosed_name()
            .context("update ZIP contains an unsafe path")?
            .to_owned();
        let output = target.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }
        total = total.saturating_add(entry.size());
        if total > MAX_UPDATE_SIZE as u64 {
            bail!("extracted CodeFace update exceeds the size limit");
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(&output)?;
        std::io::copy(&mut entry, &mut file)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&output, fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

fn current_install_path() -> Result<PathBuf> {
    let executable = std::env::current_exe()?;
    #[cfg(target_os = "macos")]
    {
        let app = executable
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .context("CodeFace is not running from an application bundle")?;
        if app.extension().and_then(|value| value.to_str()) != Some("app") {
            bail!("CodeFace is not running from an application bundle");
        }
        Ok(app.to_owned())
    }
    #[cfg(target_os = "windows")]
    {
        Ok(executable)
    }
}

fn staged_install_path(root: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        root.join("payload/CodeFace.app")
    } else {
        root.join("payload/CodeFace.exe")
    }
}

pub fn check_and_prepare_update() -> Result<UpdateOutcome> {
    let client = Client::builder()
        .user_agent(format!("CodeFace/{}", paths::VERSION))
        .timeout(Duration::from_secs(45))
        .build()?;
    let release: LatestRelease = client
        .get(LATEST_RELEASE_URL)
        .send()
        .context("failed to check the latest CodeFace release")?
        .error_for_status()
        .context("GitHub latest release request failed")?
        .json()
        .context("GitHub latest release response is invalid")?;
    let latest = parse_version(&release.tag_name)?;
    if latest <= parse_version(paths::VERSION)? {
        return Ok(UpdateOutcome::UpToDate);
    }

    let asset_name = platform_asset_name();
    let checksum_name = format!("{asset_name}.sha256");
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .with_context(|| format!("latest release is missing {asset_name}"))?;
    let checksum = release
        .assets
        .iter()
        .find(|asset| asset.name == checksum_name)
        .with_context(|| format!("latest release is missing {checksum_name}"))?;
    let expected = expected_checksum(&download(&client, &checksum.browser_download_url, 4096)?)?;
    let archive = download(&client, &asset.browser_download_url, MAX_UPDATE_SIZE)?;
    verify_checksum(&archive, &expected)?;

    let staging = paths::state_root()?.join("updates").join(format!(
        "{}-{}",
        release.tag_name.trim_start_matches('v'),
        std::process::id()
    ));
    extract_zip(&archive, &staging.join("payload"))?;
    let staged = staged_install_path(&staging);
    if !staged.is_file() && !staged.is_dir() {
        bail!("CodeFace update archive does not contain {asset_name}");
    }
    let current = current_install_path()?;
    let helper = staging.join(if cfg!(target_os = "windows") {
        "update-helper.exe"
    } else {
        "update-helper"
    });
    fs::copy(std::env::current_exe()?, &helper)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o755))?;
    }
    Command::new(&helper)
        .arg("--complete-update")
        .arg(&current)
        .arg(&staged)
        .arg(std::process::id().to_string())
        .arg(&staging)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to start the CodeFace update helper")?;
    Ok(UpdateOutcome::Restarting)
}

fn process_running(pid: u32) -> bool {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    system.process(sysinfo::Pid::from_u32(pid)).is_some()
}

fn wait_for_exit(pid: u32) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if !process_running(pid) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(150));
    }
    bail!("CodeFace did not exit before the update timeout")
}

pub fn complete_update(arguments: &[String]) -> Result<()> {
    let current = PathBuf::from(arguments.first().context("missing current install path")?);
    let staged = PathBuf::from(arguments.get(1).context("missing staged install path")?);
    let parent_pid = arguments
        .get(2)
        .context("missing update parent PID")?
        .parse::<u32>()?;
    let staging = PathBuf::from(arguments.get(3).context("missing update staging path")?);
    let result = (|| -> Result<()> {
        wait_for_exit(parent_pid)?;
        cdp::pause_control_for_update()?;

        let backup =
            current.with_extension(format!("codeface-update-backup-{}", std::process::id()));
        if backup.exists() {
            if backup.is_dir() {
                fs::remove_dir_all(&backup)?;
            } else {
                fs::remove_file(&backup)?;
            }
        }
        fs::rename(&current, &backup).context("failed to back up the current CodeFace install")?;
        let install_result = if cfg!(target_os = "macos") {
            fs::rename(&staged, &current)
        } else {
            fs::copy(&staged, &current).map(|_| ())
        };
        if let Err(error) = install_result {
            fs::rename(&backup, &current).ok();
            return Err(error).context("failed to install the new CodeFace version");
        }

        let executable = installed_executable(&current);
        let control = Command::new(&executable)
            .arg("--resume-control-after-update")
            .status()
            .context("failed to start the updated CodeFace control bridge")?;
        if !control.success() {
            if current.is_dir() {
                fs::remove_dir_all(&current).ok();
            } else {
                fs::remove_file(&current).ok();
            }
            fs::rename(&backup, &current).ok();
            bail!("updated CodeFace could not restore its control bridge");
        }
        if let Err(error) = Command::new(&executable).spawn() {
            if current.is_dir() {
                fs::remove_dir_all(&current).ok();
            } else {
                fs::remove_file(&current).ok();
            }
            fs::rename(&backup, &current).ok();
            return Err(error).context("failed to restart the updated CodeFace app");
        }
        if backup.is_dir() {
            fs::remove_dir_all(backup).ok();
        } else {
            fs::remove_file(backup).ok();
        }
        fs::remove_dir_all(&staging).ok();
        Ok(())
    })();
    if let Err(error) = &result {
        fs::write(staging.join("update-error.log"), format!("{error:#}\n")).ok();
        if !process_running(parent_pid) && current.exists() {
            let executable = installed_executable(&current);
            Command::new(&executable)
                .arg("--resume-control-after-update")
                .status()
                .ok();
            Command::new(executable).spawn().ok();
        }
    }
    result
}

fn installed_executable(install: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        install.join("Contents/MacOS/CodeFace")
    } else {
        install.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{ZipWriter, write::SimpleFileOptions};

    #[test]
    fn versions_are_compared_numerically() {
        assert!(parse_version("v1.10.0").unwrap() > parse_version("1.9.9").unwrap());
        assert!(parse_version("1.5.1").is_ok());
        assert!(parse_version("1.5").is_err());
    }

    #[test]
    fn checksum_files_are_strict() {
        let value = format!("{}  CodeFace-macOS.zip\n", "a".repeat(64));
        assert_eq!(expected_checksum(value.as_bytes()).unwrap(), "a".repeat(64));
        assert!(expected_checksum(b"not-a-checksum").is_err());
    }

    #[test]
    fn update_archives_reject_parent_traversal() {
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        archive
            .start_file("../outside", SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"unsafe").unwrap();
        let bytes = archive.finish().unwrap().into_inner();
        let root =
            std::env::temp_dir().join(format!("codeface-update-test-{}", std::process::id()));
        let error = extract_zip(&bytes, &root).unwrap_err();
        assert!(error.to_string().contains("unsafe path"));
        fs::remove_dir_all(root).ok();
    }
}
