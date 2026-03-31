use ed25519_dalek::{SigningKey, pkcs8::EncodePublicKey};
use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
use rand::rngs::OsRng;
use tokio::sync::OnceCell;

/// 生成 ED25519 密钥对，返回 (私钥PEM, 公钥PEM)
pub fn generate_ed25519_keypair() -> (String, String) {
    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    // 私钥：32字节裸数据 → 十六进制字符串
    let private_hex = signing_key
        .to_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();

    // 公钥：标准 SPKI PEM
    let public_pem = verifying_key
        .to_public_key_pem(LineEnding::LF)
        .expect("公钥编码失败");

    (private_hex, public_pem)
}


static BINANCE_API_KEY: OnceCell<String> = OnceCell::const_new();

pub async fn get_api_key() -> &'static String {
    BINANCE_API_KEY
        .get_or_init(|| async {
            std::env::var("binance_test_public_key")
                .expect("环境变量 binance_test_public_key 未设置")
        })
        .await
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_generate_ed25519_keypair() {
        let (private_pem, public_pem) = generate_ed25519_keypair();
        println!("{}", private_pem);
        println!("{}", public_pem);
    }
}
