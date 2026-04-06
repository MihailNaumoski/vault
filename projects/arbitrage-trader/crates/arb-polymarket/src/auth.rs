use base64::{engine::general_purpose::STANDARD, Engine as _};
use hmac::{Hmac, Mac};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use sha2::Sha256;

use crate::error::PolymarketError;

type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256 request signer for the Polymarket CLOB API.
pub struct PolyAuth {
    api_key: String,
    secret: Vec<u8>, // base64-decoded secret
    passphrase: String,
}

impl PolyAuth {
    /// Create a new authenticator from raw credentials.
    ///
    /// `secret_b64` is the base64-encoded HMAC secret.
    pub fn new(
        api_key: String,
        secret_b64: String,
        passphrase: String,
    ) -> Result<Self, PolymarketError> {
        let secret = STANDARD
            .decode(&secret_b64)
            .map_err(|e| PolymarketError::Auth(format!("invalid base64 secret: {e}")))?;
        Ok(Self {
            api_key,
            secret,
            passphrase,
        })
    }

    /// Sign a request, returning `(timestamp, signature)`.
    ///
    /// Signature = `Base64(HMAC-SHA256(timestamp + METHOD + path + body, secret))`
    pub fn sign_request(&self, method: &str, path: &str, body: &str) -> (String, String) {
        let timestamp = chrono::Utc::now().timestamp().to_string();
        let signature = self.sign_with_timestamp(&timestamp, method, path, body);
        (timestamp, signature)
    }

    /// Internal: compute signature for a given timestamp (useful for testing).
    pub(crate) fn sign_with_timestamp(
        &self,
        timestamp: &str,
        method: &str,
        path: &str,
        body: &str,
    ) -> String {
        let message = format!("{}{}{}{}", timestamp, method.to_uppercase(), path, body);
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts any key length");
        mac.update(message.as_bytes());
        let result = mac.finalize().into_bytes();
        STANDARD.encode(result)
    }

    /// Build the complete auth header map for a request.
    ///
    /// Headers: `POLY_API_KEY`, `POLY_SIGNATURE`, `POLY_TIMESTAMP`, `POLY_PASSPHRASE`.
    pub fn headers(
        &self,
        method: &str,
        path: &str,
        body: &str,
    ) -> Result<HeaderMap, PolymarketError> {
        let (timestamp, signature) = self.sign_request(method, path, body);
        let mut map = HeaderMap::new();
        map.insert(
            HeaderName::from_static("poly_api_key"),
            HeaderValue::from_str(&self.api_key)
                .map_err(|e| PolymarketError::Auth(format!("invalid api_key header value: {e}")))?,
        );
        map.insert(
            HeaderName::from_static("poly_signature"),
            HeaderValue::from_str(&signature)
                .map_err(|e| PolymarketError::Auth(format!("invalid signature header value: {e}")))?,
        );
        map.insert(
            HeaderName::from_static("poly_timestamp"),
            HeaderValue::from_str(&timestamp)
                .map_err(|e| PolymarketError::Auth(format!("invalid timestamp header value: {e}")))?,
        );
        map.insert(
            HeaderName::from_static("poly_passphrase"),
            HeaderValue::from_str(&self.passphrase)
                .map_err(|e| PolymarketError::Auth(format!("invalid passphrase header value: {e}")))?,
        );
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_auth() -> PolyAuth {
        // A known secret (base64-encoded)
        let secret_b64 = STANDARD.encode(b"test-secret-key-1234");
        PolyAuth::new(
            "test-api-key".to_string(),
            secret_b64,
            "test-passphrase".to_string(),
        )
        .unwrap()
    }

    #[test]
    fn test_sign_request_known_vector() {
        let auth = test_auth();
        let sig = auth.sign_with_timestamp("1700000000", "GET", "/markets", "");

        // Re-compute expected
        let message = "1700000000GET/markets";
        let mut mac =
            HmacSha256::new_from_slice(b"test-secret-key-1234").unwrap();
        mac.update(message.as_bytes());
        let expected = STANDARD.encode(mac.finalize().into_bytes());

        assert_eq!(sig, expected);
    }

    #[test]
    fn test_sign_request_empty_body() {
        let auth = test_auth();
        let sig1 = auth.sign_with_timestamp("123", "GET", "/foo", "");
        let sig2 = auth.sign_with_timestamp("123", "DELETE", "/foo", "");
        // Different methods produce different signatures
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_sign_request_with_body() {
        let auth = test_auth();
        let sig_no_body = auth.sign_with_timestamp("123", "POST", "/order", "");
        let sig_with_body =
            auth.sign_with_timestamp("123", "POST", "/order", r#"{"price":"0.50"}"#);
        assert_ne!(sig_no_body, sig_with_body);
    }

    #[test]
    fn test_headers_correct_keys() {
        let auth = test_auth();
        let headers = auth.headers("GET", "/markets", "").unwrap();

        assert!(headers.contains_key("poly_api_key"));
        assert!(headers.contains_key("poly_signature"));
        assert!(headers.contains_key("poly_timestamp"));
        assert!(headers.contains_key("poly_passphrase"));
        assert_eq!(headers.len(), 4);

        assert_eq!(headers["poly_api_key"], "test-api-key");
        assert_eq!(headers["poly_passphrase"], "test-passphrase");
    }

    #[test]
    fn test_method_case_sensitivity() {
        let auth = test_auth();
        let sig_lower = auth.sign_with_timestamp("1", "get", "/x", "");
        let sig_upper = auth.sign_with_timestamp("1", "GET", "/x", "");
        // sign_with_timestamp uppercases internally, so they should match
        assert_eq!(sig_lower, sig_upper);
    }
}
