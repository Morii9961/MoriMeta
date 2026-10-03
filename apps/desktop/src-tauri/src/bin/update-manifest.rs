// SPDX-License-Identifier: GPL-3.0-or-later
//! Offline release preparation. No private keys, network requests or publication.
#[path = "../updater_artifact.rs"]
mod updater_artifact;

use base64::Engine as _;
use std::io::Write;
use std::path::Path;

fn main() {
    if let Err(error) = run(std::env::args().skip(1).collect()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn manifest(
    installer: &Path,
    signature: &Path,
    key: &Path,
    version: &str,
    previous: &str,
    notes: &Path,
) -> Result<serde_json::Value, String> {
    let version = semver::Version::parse(version)
        .map_err(|e| e.to_string())?
        .to_string();
    let filename = installer
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("the installer needs a UTF-8 filename")?;
    if !filename.to_ascii_lowercase().ends_with(".exe")
        || !filename
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err("the installer filename must be an ASCII .exe name".into());
    }
    if std::fs::metadata(installer)
        .map_err(|e| e.to_string())?
        .len()
        > updater_artifact::MAX_INSTALLER_BYTES
    {
        return Err("the update installer is too large".into());
    }
    let bytes = std::fs::read(installer).map_err(|e| e.to_string())?;
    let signature = std::fs::read(signature).map_err(|e| e.to_string())?;
    let key = std::fs::read(key).map_err(|e| e.to_string())?;
    let encode = |b| base64::engine::general_purpose::STANDARD.encode(b);
    let signature = encode(&signature);
    updater_artifact::validate_artifact(&bytes, &encode(&key), &signature, &version, previous)?;
    let notes = std::fs::read_to_string(notes).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "version": version,
        "notes": notes,
        "platforms": {
            "windows-x86_64": {
                "url": format!("https://github.com/Morii9961/MoriMeta/releases/download/v{version}/{filename}"),
                "signature": signature,
            }
        }
    }))
}

fn run(args: Vec<String>) -> Result<(), String> {
    if args.len() != 7 {
        return Err("usage: update-manifest INSTALLER.exe SIGNATURE.minisig PUBLIC_KEY.pub VERSION PREVIOUS_VERSION NOTES.txt OUTPUT.json".into());
    }
    let json = manifest(
        Path::new(&args[0]),
        Path::new(&args[1]),
        Path::new(&args[2]),
        &args[3],
        &args[4],
        Path::new(&args[5]),
    )?;
    let output = Path::new(&args[6]);
    if output.exists() {
        return Err("the output already exists; choose a new output file".into());
    }
    let name = output
        .file_name()
        .ok_or("the output needs a filename")?
        .to_string_lossy();
    let partial = output.with_file_name(format!("{name}.partial"));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&partial)
        .map_err(|e| e.to_string())?;
    file.write_all(
        serde_json::to_string_pretty(&json)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    // Both operations are on the same directory/volume; the destination is never replaced.
    #[cfg(windows)]
    std::fs::rename(&partial, output).map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    {
        std::fs::hard_link(&partial, output).map_err(|e| e.to_string())?;
        std::fs::remove_file(&partial).map_err(|e| e.to_string())?;
    }
    println!("Verified manifest written; upload and publish remain separate operator actions.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_verified_newer_artifact_produces_a_manifest_and_existing_output_is_kept() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/updater.json")).unwrap();
        let root = std::env::temp_dir().join(format!(
            "mm-update-manifest-{}-{}",
            std::process::id(),
            mm_store::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let installer = root.join("MoriMeta-test.exe");
        let signature = root.join("test.minisig");
        let key = root.join("test.pub");
        let notes = root.join("notes.txt");
        std::fs::write(&installer, fixture["payload"].as_str().unwrap()).unwrap();
        let decode = |s: &str| {
            base64::engine::general_purpose::STANDARD
                .decode(fixture[s].as_str().unwrap())
                .unwrap()
        };
        std::fs::write(&signature, decode("signature")).unwrap();
        std::fs::write(&key, decode("publicKey")).unwrap();
        std::fs::write(&notes, "Synthetic fixture; not an installer.").unwrap();
        let json = manifest(&installer, &signature, &key, "0.2.0", "0.1.0", &notes).unwrap();
        assert_eq!(
            json["platforms"]["windows-x86_64"]["url"],
            "https://github.com/Morii9961/MoriMeta/releases/download/v0.2.0/MoriMeta-test.exe"
        );
        assert!(manifest(&installer, &signature, &key, "9.9.9", "0.1.0", &notes).is_err());
        assert!(manifest(&installer, &signature, &key, "0.2.0", "0.2.0", &notes).is_err());
        let output = root.join("latest.json");
        let args = vec![installer, signature, key]
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .chain([
                "0.2.0".into(),
                "0.1.0".into(),
                notes.to_string_lossy().into_owned(),
                output.to_string_lossy().into_owned(),
            ])
            .collect::<Vec<_>>();
        run(args.clone()).unwrap();
        let before = std::fs::read(&output).unwrap();
        assert!(run(args.clone()).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), before);
        std::fs::write(root.join("MoriMeta-test.exe"), "tampered").unwrap();
        assert!(run(args).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
