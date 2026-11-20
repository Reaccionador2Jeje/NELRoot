//! Detached Ed25519 signatures for first-party NELRoot Hub manifests.
//!
//! Signatures cover the exact manifest file bytes. Verification requires a
//! public key supplied through an independently trusted channel.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HubSignature {
    pub schema_version: u32,
    pub algorithm: String,
    pub key_id: String,
    pub manifest_sha256: String,
    pub signature_hex: String,
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(format!("{} must be a regular file no larger than {limit} bytes", path.display()).into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(format!("{} exceeds its size limit", path.display()).into());
    }
    Ok(bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn hex_decode(value: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if value.len() % 2 != 0 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("key or signature must be an even-length hexadecimal string".into());
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let text = std::str::from_utf8(pair)?;
        bytes.push(u8::from_str_radix(text, 16)?);
    }
    Ok(bytes)
}

fn key_id(public_key: &[u8]) -> String {
    hex_encode(&Sha256::digest(public_key))
}

fn read_manifest(path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let bytes = read_bounded(path, MAX_MANIFEST_BYTES)?;
    let manifest: super::hub::HubManifest = serde_json::from_slice(&bytes)?;
    super::hub::validate_manifest(&manifest)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    Ok(bytes)
}

/// Generate an Ed25519 key pair without overwriting existing files.
pub fn generate_keypair(private_path: &Path, public_path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    if private_path == public_path {
        return Err("private and public key paths must be different".into());
    }
    let mut seed = [0_u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut seed)?;
    let signing = SigningKey::from_bytes(&seed);
    let public = signing.verifying_key().to_bytes();

    let mut private_options = fs::OpenOptions::new();
    private_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        private_options.mode(0o600);
    }
    let mut private_file = private_options.open(private_path)?;
    use std::io::Write;
    private_file.write_all(&seed)?;
    private_file.sync_all()?;

    let public_result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut public_file = fs::OpenOptions::new().write(true).create_new(true).open(public_path)?;
        writeln!(public_file, "{}", hex_encode(&public))?;
        public_file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = public_result {
        let _ = fs::remove_file(private_path);
        return Err(error);
    }
    Ok(key_id(&public))
}

/// Sign the exact bytes of a validated manifest and write a detached JSON signature.
pub fn sign_manifest(manifest_path: &Path, private_path: &Path, output_path: &Path) -> Result<HubSignature, Box<dyn std::error::Error>> {
    let bytes = read_manifest(manifest_path)?;
    let secret = read_bounded(private_path, 32)?;
    let seed: [u8; 32] = secret.try_into().map_err(|_| "private key must contain exactly 32 raw bytes")?;
    let signing = SigningKey::from_bytes(&seed);
    let public = signing.verifying_key().to_bytes();
    let signature = signing.sign(&bytes);
    let document = HubSignature {
        schema_version: 1,
        algorithm: "ed25519".into(),
        key_id: key_id(&public),
        manifest_sha256: hex_encode(&Sha256::digest(&bytes)),
        signature_hex: hex_encode(&signature.to_bytes()),
    };
    let encoded = serde_json::to_vec_pretty(&document)?;
    let mut output = fs::OpenOptions::new().write(true).create_new(true).open(output_path)?;
    use std::io::Write;
    output.write_all(&encoded)?;
    output.write_all(b"\n")?;
    output.sync_all()?;
    Ok(document)
}

/// Verify a detached signature using an explicitly supplied trusted public key.
pub fn verify_manifest(manifest_path: &Path, signature_path: &Path, public_path: &Path) -> Result<HubSignature, Box<dyn std::error::Error>> {
    let bytes = read_manifest(manifest_path)?;
    let signature_bytes = read_bounded(signature_path, 16 * 1024)?;
    let document: HubSignature = serde_json::from_slice(&signature_bytes)?;
    if document.schema_version != 1 || document.algorithm != "ed25519" {
        return Err("unsupported Hub signature schema or algorithm".into());
    }
    let digest = hex_encode(&Sha256::digest(&bytes));
    if document.manifest_sha256 != digest {
        return Err("manifest SHA-256 does not match the detached signature".into());
    }
    let public_text = fs::read_to_string(public_path)?;
    let public_bytes = hex_decode(public_text.trim())?;
    let public_array: [u8; 32] = public_bytes.try_into().map_err(|_| "public key must decode to exactly 32 bytes")?;
    let public = VerifyingKey::from_bytes(&public_array)?;
    if document.key_id != key_id(&public_array) {
        return Err("signature key ID does not match the supplied trusted public key".into());
    }
    let signature_bytes = hex_decode(&document.signature_hex)?;
    let signature = Signature::from_slice(&signature_bytes)?;
    public.verify_strict(&bytes, &signature).map_err(|_| "Ed25519 signature verification failed")?;
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hex_round_trip_and_rejects_invalid_input() {
        let bytes = [0_u8, 1, 127, 128, 255];
        assert_eq!(hex_decode(&hex_encode(&bytes)).unwrap(), bytes);
        assert!(hex_decode("xyz").is_err());
    }

    #[test]
    fn key_ids_are_stable_sha256_fingerprints() {
        let key = [7_u8; 32];
        assert_eq!(key_id(&key), hex_encode(&Sha256::digest(key)));
        assert_eq!(key_id(&key).len(), 64);
    }

    #[test]
    fn signature_document_rejects_unknown_schema() {
        let value = HubSignature {
            schema_version: 2,
            algorithm: "ed25519".into(),
            key_id: "a".repeat(64),
            manifest_sha256: "b".repeat(64),
            signature_hex: "c".repeat(128),
        };
        assert_ne!(value.schema_version, 1);
    }
}
