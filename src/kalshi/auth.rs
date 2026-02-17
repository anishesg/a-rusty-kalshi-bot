use crate::errors::{EngineError, EngineResult};
use base64::Engine as _;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs1v15::SigningKey;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::RsaPrivateKey;
use sha2::Sha256;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Kalshi API authenticator using RSA PKCS#1 v1.5 signatures.
/// Produces three headers per request: key ID, timestamp (ms), and signature.
#[derive(Clone)]
pub struct KalshiAuth {
    api_key_id: String,
    signing_key: SigningKey<Sha256>,
}

impl KalshiAuth {
    pub fn new(api_key_id: &str, private_key_path: &Path) -> EngineResult<Self> {
        // Try env var first (for Railway/cloud), fall back to file
        let pem = if let Ok(pem_env) = std::env::var("KALSHI_PRIVATE_KEY_PEM") {
            tracing::info!("loaded RSA key from KALSHI_PRIVATE_KEY_PEM env var");
            pem_env
        } else {
            tracing::info!(path = %private_key_path.display(), "loading RSA key from file");
            std::fs::read_to_string(private_key_path)
                .map_err(|e| EngineError::Auth(format!("read key {}: {e}", private_key_path.display())))?
        };

        let private_key = RsaPrivateKey::from_pkcs1_pem(&pem)
            .map_err(|e| EngineError::Auth(format!("parse RSA PEM: {e}")))?;

        let signing_key = SigningKey::<Sha256>::new(private_key);

        Ok(Self {
            api_key_id: api_key_id.to_string(),
            signing_key,
        })
    }

    /// Returns (key_id, timestamp_ms, base64_signature) for the given request.
    pub fn sign_request(&self, method: &str, path: &str, body: &str) -> EngineResult<(String, String, String)> {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| EngineError::Auth(format!("system clock: {e}")))?
            .as_millis()
            .to_string();

        // Kalshi signing payload: timestamp + METHOD + path + body
        let message = format!("{}{}{}{}", timestamp_ms, method.to_uppercase(), path, body);

        let signature = self.signing_key.sign(message.as_bytes());
        let sig_b64 = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());

        Ok((self.api_key_id.clone(), timestamp_ms, sig_b64))
    }
}
