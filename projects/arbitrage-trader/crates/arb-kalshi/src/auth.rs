use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::header::{HeaderMap, HeaderValue};
use rsa::pkcs8::DecodePrivateKey;
use rsa::pss::BlindedSigningKey;
use rsa::signature::{RandomizedSigner, SignatureEncoding};
use rsa::RsaPrivateKey;
use sha2::Sha256;

use crate::error::KalshiError;

/// Handles RSA-PSS-SHA256 request signing for the Kalshi API.
///
/// Each request is signed with: `RSA_PSS_SHA256(timestamp_ms + METHOD + path, private_key)`.
/// Note: unlike Polymarket, the request body is NOT included in the signing message.
#[derive(Clone)]
pub struct KalshiAuth {
    api_key_id: String,
    private_key: RsaPrivateKey,
}

impl KalshiAuth {
    /// Create from API key ID and PEM-encoded RSA private key (PKCS#8).
    pub fn new(api_key_id: String, private_key_pem: &str) -> Result<Self, KalshiError> {
        let private_key = RsaPrivateKey::from_pkcs8_pem(private_key_pem)
            .map_err(|e| KalshiError::Auth(format!("failed to load PEM key: {e}")))?;
        Ok(Self {
            api_key_id,
            private_key,
        })
    }

    /// Return the API key ID.
    pub fn api_key_id(&self) -> &str {
        &self.api_key_id
    }

    /// Sign a request for the given method and path.
    ///
    /// Returns `(timestamp_ms_string, base64_signature)`.
    /// The message signed is: `timestamp_ms + METHOD + path` (no body).
    pub fn sign_request(&self, method: &str, path: &str) -> (String, String) {
        let timestamp_ms = chrono::Utc::now().timestamp_millis().to_string();
        let signature = self.sign_with_timestamp(&timestamp_ms, method, path);
        (timestamp_ms, signature)
    }

    /// Sign with a specific timestamp (useful for testing).
    ///
    /// RSA-PSS signatures are non-deterministic: the same input produces
    /// different signatures each time due to randomized salt.
    pub(crate) fn sign_with_timestamp(
        &self,
        timestamp_ms: &str,
        method: &str,
        path: &str,
    ) -> String {
        let message = format!("{}{}{}", timestamp_ms, method.to_uppercase(), path);
        let signing_key = BlindedSigningKey::<Sha256>::new(self.private_key.clone());
        let mut rng = rand::thread_rng();
        let sig = signing_key.sign_with_rng(&mut rng, message.as_bytes());
        STANDARD.encode(sig.to_bytes())
    }

    /// Build the auth headers for a request.
    ///
    /// Headers: `KALSHI-ACCESS-KEY`, `KALSHI-ACCESS-SIGNATURE`, `KALSHI-ACCESS-TIMESTAMP`.
    pub fn headers(&self, method: &str, path: &str) -> Result<HeaderMap, KalshiError> {
        let (timestamp, signature) = self.sign_request(method, path);
        let mut headers = HeaderMap::new();
        headers.insert(
            "KALSHI-ACCESS-KEY",
            HeaderValue::from_str(&self.api_key_id)
                .map_err(|e| KalshiError::Auth(format!("invalid api_key header value: {e}")))?,
        );
        headers.insert(
            "KALSHI-ACCESS-SIGNATURE",
            HeaderValue::from_str(&signature)
                .map_err(|e| KalshiError::Auth(format!("invalid signature header value: {e}")))?,
        );
        headers.insert(
            "KALSHI-ACCESS-TIMESTAMP",
            HeaderValue::from_str(&timestamp)
                .map_err(|e| KalshiError::Auth(format!("invalid timestamp header value: {e}")))?,
        );
        Ok(headers)
    }
}

impl std::fmt::Debug for KalshiAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KalshiAuth")
            .field("api_key_id", &self.api_key_id)
            .field("private_key", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::pkcs8::EncodePrivateKey;
    use rsa::pss::VerifyingKey;
    use rsa::signature::Verifier;

    /// Generate a test RSA key pair. Returns (PEM string, private key).
    fn test_key_pair() -> (String, RsaPrivateKey) {
        let mut rng = rand::thread_rng();
        let key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let pem = key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .unwrap();
        (pem.to_string(), key)
    }

    #[test]
    fn test_new_from_pem() {
        let (pem, _) = test_key_pair();
        let auth = KalshiAuth::new("test-key-id".to_string(), &pem);
        assert!(auth.is_ok());
        assert_eq!(auth.unwrap().api_key_id(), "test-key-id");
    }

    #[test]
    fn test_new_invalid_pem() {
        let result = KalshiAuth::new("test-key-id".to_string(), "not-a-valid-pem");
        assert!(result.is_err());
        match result.unwrap_err() {
            KalshiError::Auth(msg) => assert!(msg.contains("failed to load PEM key")),
            other => panic!("expected Auth error, got: {other:?}"),
        }
    }

    #[test]
    fn test_sign_request_verifiable() {
        let (pem, private_key) = test_key_pair();
        let auth = KalshiAuth::new("key-1".to_string(), &pem).unwrap();

        let ts = "1700000000000";
        let method = "POST";
        let path = "/trade-api/v2/portfolio/orders";
        let sig_b64 = auth.sign_with_timestamp(ts, method, path);

        // Verify with the public key using PSS
        let public_key = private_key.to_public_key();
        let verifying_key = VerifyingKey::<Sha256>::new(public_key);
        let sig_bytes = STANDARD.decode(&sig_b64).unwrap();
        let signature = rsa::pss::Signature::try_from(sig_bytes.as_slice()).unwrap();

        let message = format!("{}{}{}", ts, method.to_uppercase(), path);
        assert!(verifying_key.verify(message.as_bytes(), &signature).is_ok());
    }

    #[test]
    fn test_sign_request_method_uppercase() {
        let (pem, private_key) = test_key_pair();
        let auth = KalshiAuth::new("key-1".to_string(), &pem).unwrap();

        let ts = "1700000000000";
        let path = "/trade-api/v2/markets";
        // Lowercase method should produce a valid signature for the uppercase message
        let sig_lower = auth.sign_with_timestamp(ts, "get", path);
        let sig_upper = auth.sign_with_timestamp(ts, "GET", path);

        // PSS is non-deterministic, so signatures differ, but both should verify
        let public_key = private_key.to_public_key();
        let verifying_key = VerifyingKey::<Sha256>::new(public_key);
        let message = format!("{}GET{}", ts, path);

        for sig_b64 in &[sig_lower, sig_upper] {
            let sig_bytes = STANDARD.decode(sig_b64).unwrap();
            let signature = rsa::pss::Signature::try_from(sig_bytes.as_slice()).unwrap();
            assert!(verifying_key.verify(message.as_bytes(), &signature).is_ok());
        }
    }

    #[test]
    fn test_headers_correct_keys() {
        let (pem, _) = test_key_pair();
        let auth = KalshiAuth::new("my-api-key".to_string(), &pem).unwrap();

        let headers = auth.headers("GET", "/trade-api/v2/markets").unwrap();

        assert!(headers.contains_key("KALSHI-ACCESS-KEY"));
        assert!(headers.contains_key("KALSHI-ACCESS-SIGNATURE"));
        assert!(headers.contains_key("KALSHI-ACCESS-TIMESTAMP"));
        assert_eq!(
            headers.get("KALSHI-ACCESS-KEY").unwrap().to_str().unwrap(),
            "my-api-key"
        );
        // Signature should be non-empty base64
        let sig = headers
            .get("KALSHI-ACCESS-SIGNATURE")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(!sig.is_empty());
        assert!(STANDARD.decode(sig).is_ok());
        // Timestamp should be a number
        let ts = headers
            .get("KALSHI-ACCESS-TIMESTAMP")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ts.parse::<u64>().is_ok());
    }

    #[test]
    fn test_no_body_in_signing() {
        // Verify that signing doesn't depend on any body — only timestamp + method + path.
        // With PSS, signatures are non-deterministic, but both should verify.
        let (pem, private_key) = test_key_pair();
        let auth = KalshiAuth::new("key-1".to_string(), &pem).unwrap();

        let ts = "1700000000000";
        let path = "/trade-api/v2/portfolio/orders";
        let sig1 = auth.sign_with_timestamp(ts, "POST", path);
        let sig2 = auth.sign_with_timestamp(ts, "POST", path);

        // Both should verify against the same message (body is not a parameter)
        let public_key = private_key.to_public_key();
        let verifying_key = VerifyingKey::<Sha256>::new(public_key);
        let message = format!("{}POST{}", ts, path);

        for sig_b64 in &[sig1, sig2] {
            let sig_bytes = STANDARD.decode(sig_b64).unwrap();
            let signature = rsa::pss::Signature::try_from(sig_bytes.as_slice()).unwrap();
            assert!(verifying_key.verify(message.as_bytes(), &signature).is_ok());
        }
    }
}
