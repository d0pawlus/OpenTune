use std::{error::Error, fs, path::Path};

use base64::{engine::general_purpose::STANDARD, Engine};
use minisign_verify::{PublicKey, Signature};

pub fn verify_tauri_signature(
    public_key: &str,
    artifact: &[u8],
    tauri_signature: &str,
) -> Result<(), Box<dyn Error>> {
    let public_key = STANDARD.decode(public_key.trim())?;
    let public_key = std::str::from_utf8(&public_key)?;
    let signature = STANDARD.decode(tauri_signature.trim())?;
    let signature = std::str::from_utf8(&signature)?;
    let public_key = PublicKey::decode(public_key)?;
    let signature = Signature::decode(signature)?;

    public_key.verify(artifact, &signature, true)?;
    Ok(())
}

pub fn verify_files(
    public_key_path: &Path,
    artifact_path: &Path,
    signature_path: &Path,
) -> Result<(), Box<dyn Error>> {
    let public_key = fs::read_to_string(public_key_path)?;
    let artifact = fs::read(artifact_path)?;
    let signature = fs::read_to_string(signature_path)?;

    verify_tauri_signature(&public_key, &artifact, &signature)
}

#[cfg(test)]
mod tests {
    use super::verify_tauri_signature;

    const MINISIGN_PUBLIC_KEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\n\
RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const SIGNATURE: &str = "untrusted comment: signature from minisign secret key\n\
RUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\n\
trusted comment: timestamp:1556193335\tfile:test\n\
y/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";

    fn tauri_wrap(value: &str) -> String {
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, value)
    }

    #[test]
    fn accepts_tauri_wrapped_signature() {
        let public_key = tauri_wrap(MINISIGN_PUBLIC_KEY);
        let signature = tauri_wrap(SIGNATURE);

        verify_tauri_signature(&public_key, b"test", &signature).unwrap();
    }

    #[test]
    fn rejects_modified_artifact() {
        let public_key = tauri_wrap(MINISIGN_PUBLIC_KEY);
        let signature = tauri_wrap(SIGNATURE);

        assert!(verify_tauri_signature(&public_key, b"modified", &signature).is_err());
    }
}
