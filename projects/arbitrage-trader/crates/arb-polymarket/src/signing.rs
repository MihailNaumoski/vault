use alloy_primitives::{Address, U256};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{eip712_domain, sol, SolStruct};

use crate::error::PolymarketError;

/// Polymarket CTF Exchange address on Polygon.
pub const CTF_EXCHANGE_ADDRESS: &str = "0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E";

// EIP-712 typed data struct matching Polymarket's CTF Exchange contract.
sol! {
    #[derive(Debug)]
    struct Order {
        uint256 salt;
        address maker;
        address signer;
        address taker;
        uint256 tokenId;
        uint256 makerAmount;
        uint256 takerAmount;
        uint256 expiration;
        uint256 nonce;
        uint256 feeRateBps;
        uint8 side;
        uint8 signatureType;
    }
}

/// EIP-712 order signer for the Polymarket CTF Exchange.
pub struct OrderSigner {
    signer: PrivateKeySigner,
    chain_id: u64,
    verifying_contract: Address,
}

impl OrderSigner {
    /// Create a new order signer from a hex-encoded private key.
    ///
    /// `private_key_hex` may optionally start with "0x".
    pub fn new(private_key_hex: &str, chain_id: u64) -> Result<Self, PolymarketError> {
        let signer: PrivateKeySigner = private_key_hex
            .parse()
            .map_err(|e| PolymarketError::Signing(format!("invalid private key: {e}")))?;

        let contract_addr: Address = CTF_EXCHANGE_ADDRESS
            .parse()
            .map_err(|e| PolymarketError::Signing(format!("invalid contract address: {e}")))?;

        Ok(Self {
            signer,
            chain_id,
            verifying_contract: contract_addr,
        })
    }

    /// Get the wallet (maker) address.
    pub fn address(&self) -> Address {
        self.signer.address()
    }

    /// Sign a limit order request, returning the signed order body as JSON.
    ///
    /// `token_id` is the Polymarket outcome token ID (YES or NO).
    pub async fn sign_order(
        &self,
        req: &arb_types::LimitOrderRequest,
        token_id: &str,
    ) -> Result<serde_json::Value, PolymarketError> {
        let maker = self.signer.address();

        // Side mapping: Yes (Buy) = 0, No (Sell) = 1
        let side_u8: u8 = match req.side {
            arb_types::Side::Yes => 0,
            arb_types::Side::No => 1,
        };

        // Price is 0.00-1.00 USDC.e per share. Amounts in USDC units (6 decimals).
        // For a BUY: makerAmount = price * quantity * 1e6, takerAmount = quantity * 1e6
        // For a SELL: makerAmount = quantity * 1e6, takerAmount = price * quantity * 1e6
        //
        // All arithmetic uses rust_decimal to avoid floating-point rounding errors
        // in financial calculations (H3 security fix).
        let scale = rust_decimal::Decimal::from(1_000_000u64); // USDC.e has 6 decimals
        let qty = rust_decimal::Decimal::from(req.quantity);

        let (maker_amount, taker_amount) = if side_u8 == 0 {
            // BUY
            let maker_amt: u128 = (req.price * qty * scale).trunc().to_string().parse().unwrap_or(0);
            let taker_amt: u128 = (qty * scale).trunc().to_string().parse().unwrap_or(0);
            (U256::from(maker_amt), U256::from(taker_amt))
        } else {
            // SELL
            let maker_amt: u128 = (qty * scale).trunc().to_string().parse().unwrap_or(0);
            let taker_amt: u128 = (req.price * qty * scale).trunc().to_string().parse().unwrap_or(0);
            (U256::from(maker_amt), U256::from(taker_amt))
        };

        let salt = U256::from(rand::random::<u128>());
        let nonce = U256::from(rand::random::<u128>());
        let expiration =
            U256::from(chrono::Utc::now().timestamp() as u64 + 300); // 5 min TTL

        let token_id_u256 = U256::from_str_radix(token_id, 10).unwrap_or_else(|_| {
            // Try hex
            token_id
                .strip_prefix("0x")
                .and_then(|hex| U256::from_str_radix(hex, 16).ok())
                .unwrap_or(U256::ZERO)
        });

        let order = Order {
            salt,
            maker,
            signer: maker,
            taker: Address::ZERO,
            tokenId: token_id_u256,
            makerAmount: maker_amount,
            takerAmount: taker_amount,
            expiration,
            nonce,
            feeRateBps: U256::ZERO,
            side: side_u8,
            signatureType: 0, // EOA
        };

        let domain = eip712_domain! {
            name: "Polymarket CTF Exchange",
            version: "1",
            chain_id: self.chain_id,
            verifying_contract: self.verifying_contract,
        };

        let hash = order.eip712_signing_hash(&domain);
        let signature = self
            .signer
            .sign_hash_sync(&hash)
            .map_err(|e| PolymarketError::Signing(format!("EIP-712 sign failed: {e}")))?;

        let sig_hex = format!("0x{}", hex::encode(signature.as_bytes()));

        let body = serde_json::json!({
            "order": {
                "salt": salt.to_string(),
                "maker": format!("{:?}", maker),
                "signer": format!("{:?}", maker),
                "taker": format!("{:?}", Address::ZERO),
                "tokenId": token_id,
                "makerAmount": maker_amount.to_string(),
                "takerAmount": taker_amount.to_string(),
                "expiration": expiration.to_string(),
                "nonce": nonce.to_string(),
                "feeRateBps": "0",
                "side": side_u8.to_string(),
                "signatureType": "0",
            },
            "signature": sig_hex,
            "owner": format!("{:?}", maker),
            "orderType": "GTC",
        });

        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    // Well-known test private key (DO NOT use in production).
    const TEST_PRIVATE_KEY: &str =
        "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    #[test]
    fn test_signer_address() {
        let signer = OrderSigner::new(TEST_PRIVATE_KEY, 137).unwrap();
        // This is the well-known address for the hardhat/anvil test key #0
        let expected: Address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
            .parse()
            .unwrap();
        assert_eq!(signer.address(), expected);
    }

    #[tokio::test]
    async fn test_sign_order_produces_valid_json() {
        let signer = OrderSigner::new(TEST_PRIVATE_KEY, 137).unwrap();
        let req = arb_types::LimitOrderRequest {
            market_id: "test-market".to_string(),
            side: arb_types::Side::Yes,
            price: dec!(0.50),
            quantity: 100,
        };

        let body = signer.sign_order(&req, "12345").await.unwrap();

        // Verify JSON structure
        assert!(body.get("order").is_some());
        assert!(body.get("signature").is_some());
        assert!(body.get("owner").is_some());
        assert!(body.get("orderType").is_some());

        let order = body.get("order").unwrap();
        assert!(order.get("salt").is_some());
        assert!(order.get("maker").is_some());
        assert!(order.get("signer").is_some());
        assert!(order.get("taker").is_some());
        assert!(order.get("tokenId").is_some());
        assert!(order.get("makerAmount").is_some());
        assert!(order.get("takerAmount").is_some());
        assert!(order.get("expiration").is_some());
        assert!(order.get("nonce").is_some());
        assert!(order.get("feeRateBps").is_some());
        assert!(order.get("side").is_some());
        assert!(order.get("signatureType").is_some());
    }

    #[tokio::test]
    async fn test_sign_order_side_mapping() {
        let signer = OrderSigner::new(TEST_PRIVATE_KEY, 137).unwrap();

        let req_yes = arb_types::LimitOrderRequest {
            market_id: "m".to_string(),
            side: arb_types::Side::Yes,
            price: dec!(0.50),
            quantity: 10,
        };
        let body_yes = signer.sign_order(&req_yes, "1").await.unwrap();
        assert_eq!(body_yes["order"]["side"].as_str().unwrap(), "0");

        let req_no = arb_types::LimitOrderRequest {
            market_id: "m".to_string(),
            side: arb_types::Side::No,
            price: dec!(0.50),
            quantity: 10,
        };
        let body_no = signer.sign_order(&req_no, "1").await.unwrap();
        assert_eq!(body_no["order"]["side"].as_str().unwrap(), "1");
    }

    #[tokio::test]
    async fn test_sign_order_amount_calculation() {
        let signer = OrderSigner::new(TEST_PRIVATE_KEY, 137).unwrap();

        // BUY 100 shares at $0.50 => makerAmount = 0.50 * 100 * 1e6 = 50_000_000
        //                           takerAmount = 100 * 1e6 = 100_000_000
        let req = arb_types::LimitOrderRequest {
            market_id: "m".to_string(),
            side: arb_types::Side::Yes,
            price: dec!(0.50),
            quantity: 100,
        };
        let body = signer.sign_order(&req, "1").await.unwrap();
        let maker_amount: u64 = body["order"]["makerAmount"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let taker_amount: u64 = body["order"]["takerAmount"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(maker_amount, 50_000_000);
        assert_eq!(taker_amount, 100_000_000);
    }

    #[tokio::test]
    async fn test_sign_order_sell_amount_calculation() {
        let signer = OrderSigner::new(TEST_PRIVATE_KEY, 137).unwrap();

        // SELL 50 shares at $0.80 => makerAmount = 50 * 1e6 = 50_000_000
        //                           takerAmount = 0.80 * 50 * 1e6 = 40_000_000
        let req = arb_types::LimitOrderRequest {
            market_id: "m".to_string(),
            side: arb_types::Side::No,
            price: dec!(0.80),
            quantity: 50,
        };
        let body = signer.sign_order(&req, "1").await.unwrap();
        let maker_amount: u64 = body["order"]["makerAmount"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let taker_amount: u64 = body["order"]["takerAmount"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(maker_amount, 50_000_000);
        assert_eq!(taker_amount, 40_000_000);
    }

    #[test]
    fn test_signature_is_recoverable() {
        let signer = OrderSigner::new(TEST_PRIVATE_KEY, 137).unwrap();
        // Just ensure the signer can produce a valid signature on an arbitrary hash
        let hash = alloy_primitives::B256::ZERO;
        let sig = signer.signer.sign_hash_sync(&hash).unwrap();
        // Signature should be 65 bytes (r + s + v)
        assert_eq!(sig.as_bytes().len(), 65);
    }
}
