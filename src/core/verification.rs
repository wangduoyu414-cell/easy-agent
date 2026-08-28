use std::fs::File;
use std::io::Read;
use std::path::Path;

use base64::Engine;
use ed25519_dalek::{Signature as Ed25519Signature, VerifyingKey};
use memmap2::MmapOptions;
use minisign_verify::{PublicKey, Signature as MinisignSignature};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("updater public key is missing")]
    MissingPublicKey,
    #[error("detached signature is missing")]
    MissingSignature,
    #[error("updater public key encoding is invalid")]
    InvalidPublicKeyEncoding,
    #[error("detached signature encoding is invalid")]
    InvalidSignatureEncoding,
    #[error("both minisign and Sparkle public keys are configured")]
    ConflictingPublicKeys,
    #[error("Sparkle Ed25519 signature does not match the downloaded artifact")]
    SparkleSignatureMismatch,
    #[error("cannot verify an empty Sparkle update artifact")]
    EmptyArtifact,
    #[error(transparent)]
    Minisign(#[from] minisign_verify::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn verify_minisign_file(
    path: &Path,
    encoded_public_key_file: &str,
    signature_text: &str,
) -> Result<(), VerificationError> {
    let public_key = decode_minisign_public_key(encoded_public_key_file)?;
    let signature = MinisignSignature::decode(signature_text)?;
    let mut verifier = public_key.verify_stream(&signature)?;
    let mut file = File::open(path)?;
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        verifier.update(&buffer[..read]);
    }
    verifier.finalize()?;
    Ok(())
}

pub fn verify_minisign_bytes(
    bytes: &[u8],
    encoded_public_key_file: &str,
    signature_text: &str,
) -> Result<(), VerificationError> {
    let public_key = decode_minisign_public_key(encoded_public_key_file)?;
    let signature = MinisignSignature::decode(signature_text)?;
    let mut verifier = public_key.verify_stream(&signature)?;
    verifier.update(bytes);
    verifier.finalize()?;
    Ok(())
}

fn decode_minisign_public_key(
    encoded_public_key_file: &str,
) -> Result<PublicKey, VerificationError> {
    let public_key_document = base64::engine::general_purpose::STANDARD
        .decode(encoded_public_key_file.trim())
        .map_err(|_| VerificationError::InvalidPublicKeyEncoding)?;
    let public_key_document = String::from_utf8(public_key_document)
        .map_err(|_| VerificationError::InvalidPublicKeyEncoding)?;
    Ok(PublicKey::decode(&public_key_document)?)
}

pub fn verify_sparkle_ed25519_file(
    path: &Path,
    encoded_public_key: &str,
    encoded_signature: &str,
) -> Result<(), VerificationError> {
    let public_key = base64::engine::general_purpose::STANDARD
        .decode(encoded_public_key.trim())
        .map_err(|_| VerificationError::InvalidPublicKeyEncoding)?;
    let public_key: [u8; 32] = public_key
        .try_into()
        .map_err(|_| VerificationError::InvalidPublicKeyEncoding)?;
    let public_key = VerifyingKey::from_bytes(&public_key)
        .map_err(|_| VerificationError::InvalidPublicKeyEncoding)?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(encoded_signature.trim())
        .map_err(|_| VerificationError::InvalidSignatureEncoding)?;
    let signature = Ed25519Signature::from_slice(&signature)
        .map_err(|_| VerificationError::InvalidSignatureEncoding)?;

    let file = File::open(path)?;
    if file.metadata()?.len() == 0 {
        return Err(VerificationError::EmptyArtifact);
    }
    // SAFETY: the mapping is read-only and the File remains alive for the mapping's lifetime.
    let bytes = unsafe { MmapOptions::new().map(&file)? };
    public_key
        .verify_strict(&bytes, &signature)
        .map_err(|_| VerificationError::SparkleSignatureMismatch)
}

pub fn verify_configured_updater_signature_file(
    path: &Path,
    minisign_public_key: Option<&str>,
    sparkle_ed25519_public_key: Option<&str>,
    detached_signature: Option<&str>,
) -> Result<bool, VerificationError> {
    match (
        minisign_public_key,
        sparkle_ed25519_public_key,
        detached_signature,
    ) {
        (None, None, None) => Ok(false),
        (Some(_), Some(_), _) => Err(VerificationError::ConflictingPublicKeys),
        (None, None, Some(_)) => Err(VerificationError::MissingPublicKey),
        (Some(_), None, None) | (None, Some(_), None) => Err(VerificationError::MissingSignature),
        (Some(public_key), None, Some(signature)) => {
            verify_minisign_file(path, public_key, signature)?;
            Ok(true)
        }
        (None, Some(public_key), Some(signature)) => {
            verify_sparkle_ed25519_file(path, public_key, signature)?;
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_sparkle_ed25519_and_rejects_tampering() {
        let root = tempfile::tempdir().unwrap();
        let artifact = root.path().join("update.zip");
        std::fs::write(&artifact, [0x72]).unwrap();
        let public_key = base64::engine::general_purpose::STANDARD.encode(
            hex::decode("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c")
                .unwrap(),
        );
        let signature = base64::engine::general_purpose::STANDARD.encode(
            hex::decode(
                "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
            )
            .unwrap(),
        );

        assert!(
            verify_configured_updater_signature_file(
                &artifact,
                None,
                Some(&public_key),
                Some(&signature)
            )
            .unwrap()
        );
        std::fs::write(&artifact, [0x73]).unwrap();
        assert!(verify_sparkle_ed25519_file(&artifact, &public_key, &signature).is_err());
    }
}
