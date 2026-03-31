use ed25519_dalek::{SigningKey, pkcs8::EncodePrivateKey, pkcs8::EncodePublicKey};
use ed25519_dalek::pkcs8::spki::der::pem;
use rand::rngs::OsRng;

/// 生成 ED25519 密钥对，返回 (私钥PEM, 公钥PEM)
pub fn generate_ed25519_keypair() -> (String, String) {
    // 生成密钥对
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    // 私钥 → PKCS#8 PEM
    let private_pem = signing_key
        .to_pkcs8_pem(pem::LineEnding::LF)
        .expect("私钥编码失败")
        .to_string();

    // 公钥 → SubjectPublicKeyInfo (SPKI) PEM
    let public_pem = verifying_key
        .to_public_key_pem(pem::LineEnding::LF)
        .expect("公钥编码失败");

    (private_pem, public_pem)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_generate_ed25519_keypair() {
        let (private_pem, public_pem) = generate_ed25519_keypair();
        println!("private_pem: {}", private_pem);
        println!("public_pem: {}", public_pem);
    }
}
