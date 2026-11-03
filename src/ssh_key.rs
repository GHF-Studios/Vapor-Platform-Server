use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use base64::Engine;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug)]
pub(crate) struct SshPublicKey {
    pub(crate) canonical: String,
    pub(crate) fingerprint: String,
}

pub(crate) fn parse_ed25519_public_key(input: &str) -> Result<SshPublicKey, ()> {
    if input.len() > 2048 || input.chars().any(char::is_control) {
        return Err(());
    }
    let mut parts = input.split_whitespace();
    let algorithm = parts.next().ok_or(())?;
    let encoded = parts.next().ok_or(())?;
    if algorithm != "ssh-ed25519" {
        return Err(());
    }
    let blob = STANDARD
        .decode(encoded)
        .or_else(|_| STANDARD_NO_PAD.decode(encoded))
        .map_err(|_| ())?;
    let mut offset = 0;
    let algorithm_blob = read_string(&blob, &mut offset).ok_or(())?;
    let key_blob = read_string(&blob, &mut offset).ok_or(())?;
    if algorithm_blob != b"ssh-ed25519" || key_blob.len() != 32 || offset != blob.len() {
        return Err(());
    }
    let fingerprint_hash = Sha256::digest(&blob);
    let fingerprint = format!("SHA256:{}", STANDARD_NO_PAD.encode(fingerprint_hash));
    Ok(SshPublicKey {
        canonical: format!("ssh-ed25519 {encoded}"),
        fingerprint,
    })
}

fn read_string<'a>(blob: &'a [u8], offset: &mut usize) -> Option<&'a [u8]> {
    let header = blob.get(*offset..offset.checked_add(4)?)?;
    let length = u32::from_be_bytes(header.try_into().ok()?) as usize;
    *offset = offset.checked_add(4)?;
    let end = offset.checked_add(length)?;
    let value = blob.get(*offset..end)?;
    *offset = end;
    Some(value)
}
