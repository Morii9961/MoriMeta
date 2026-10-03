// SPDX-License-Identifier: GPL-3.0-or-later
// Shared by the app and the offline release manifest tool.
use base64::Engine as _;
pub const MAX_INSTALLER_BYTES: u64 = 128 * 1024 * 1024;

/// minisign's global signature covers the trusted comment. Verify that before reading version:
/// a genuine old installer paired with an inflated manifest version must never install.
pub fn validate_artifact(
    bytes: &[u8],
    key: &str,
    signature: &str,
    expected: &str,
    running: &str,
) -> Result<(), String> {
    if bytes.len() as u64 > MAX_INSTALLER_BYTES {
        return Err("the update installer is too large".into());
    }
    let decode = |s: &str| -> Result<String, String> {
        String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(s.trim())
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    };
    let public = minisign_verify::PublicKey::decode(&decode(key)?).map_err(|e| e.to_string())?;
    let sig = minisign_verify::Signature::decode(&decode(signature)?).map_err(|e| e.to_string())?;
    public
        .verify(bytes, &sig, false)
        .map_err(|e| e.to_string())?;
    let mut versions = sig
        .trusted_comment()
        .split('\t')
        .filter_map(|v| v.strip_prefix("version:"));
    let signed = versions
        .next()
        .ok_or("the update signature has no signed version")?;
    if versions.next().is_some() {
        return Err("the update signature has several signed versions".into());
    }
    let parse =
        |v: &str| semver::Version::parse(v.trim_start_matches('v')).map_err(|e| e.to_string());
    let signed = parse(signed)?;
    if signed != parse(expected)? || signed <= parse(running)? {
        return Err("the signed installer version does not match a newer update".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_artifacts_bind_the_version_and_refuse_tampering_and_replay() {
        // Generated with a disposable Ed25519 key; fixture contains no private key.
        let f: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/updater.json")).unwrap();
        let text = |name: &str| f[name].as_str().unwrap();
        let verify = |payload: &[u8], signature: &str, expected: &str, running: &str| {
            validate_artifact(payload, text("publicKey"), signature, expected, running)
        };
        let payload = text("payload").as_bytes();
        assert!(verify(payload, text("signature"), "0.2.0", "0.1.0").is_ok());
        assert!(verify(b"altered", text("signature"), "0.2.0", "0.1.0").is_err());
        assert!(verify(payload, text("signature"), "9.9.9", "0.1.0").is_err());
        assert!(verify(payload, text("signature"), "0.2.0", "0.2.0").is_err());
        assert!(verify(payload, text("signature"), "0.2.0", "0.3.0").is_err());
        assert!(verify(payload, text("legacy"), "0.2.0", "0.1.0").is_err());
        assert!(verify(payload, text("duplicate"), "0.2.0", "0.1.0").is_err());
        assert!(verify(payload, text("legacy_algorithm"), "0.2.0", "0.1.0").is_err());
        let signature = base64::engine::general_purpose::STANDARD
            .decode(text("signature"))
            .unwrap();
        let signature = String::from_utf8(signature)
            .unwrap()
            .replace("version:0.2.0", "version:9.9.9");
        let signature = base64::engine::general_purpose::STANDARD.encode(signature);
        assert!(verify(payload, &signature, "9.9.9", "0.1.0").is_err());
    }
}
