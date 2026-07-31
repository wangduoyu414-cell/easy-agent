use std::fs::File;
use std::io::Read;
use std::path::Path;

use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("updater public key is missing")]
    MissingPublicKey,
    #[error("detached signature is missing")]
    MissingSignature,
    #[error("updater public key encoding is invalid")]
    InvalidPublicKeyEncoding,
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
    let public_key_document = base64::engine::general_purpose::STANDARD
        .decode(encoded_public_key_file.trim())
        .map_err(|_| VerificationError::InvalidPublicKeyEncoding)?;
    let public_key_document = String::from_utf8(public_key_document)
        .map_err(|_| VerificationError::InvalidPublicKeyEncoding)?;
    let public_key = PublicKey::decode(&public_key_document)?;
    let signature = Signature::decode(signature_text)?;
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
