//! Flashbots authentication module
//! 
//! Implements EIP-191 message signing for authenticating requests to Flashbots
//! and other MEV relay services.

use hex;
use secp256k1::{Message, Secp256k1, SecretKey};
use sha3::{Digest, Keccak256};
use std::fmt;

/// Authentication error types
#[derive(Debug)]
pub enum AuthError {
    /// Invalid private key format
    InvalidKey(String),
    /// Signing operation failed
    SigningError(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthError::InvalidKey(msg) => write!(f, "Invalid key: {}", msg),
            AuthError::SigningError(msg) => write!(f, "Signing error: {}", msg),
        }
    }
}

impl std::error::Error for AuthError {}

impl From<secp256k1::Error> for AuthError {
    fn from(err: secp256k1::Error) -> Self {
        AuthError::SigningError(err.to_string())
    }
}

/// Flashbots request signer
/// 
/// Handles EIP-191 message signing for authenticating requests to MEV relays.
/// This ensures that only authorized addresses can submit bundles.
/// 
/// # Security
/// 
/// The signing key should be kept secure and never exposed in logs or error
/// messages. This signer creates signatures that prove ownership of a specific
/// Ethereum address without revealing the private key.
/// 
/// # Example
/// 
/// ```rust
/// let signer = FlashbotsSigner::new("0x...")?;
/// let signature = signer.sign_request("{\"jsonrpc\":\"2.0\"...}")?;
/// // signature format: "0xaddress:0xsignature"
/// ```
pub struct FlashbotsSigner {
    /// secp256k1 context for signing
    secp: Secp256k1<secp256k1::All>,
    /// Secret key for signing
    secret_key: SecretKey,
    /// Public address derived from the key
    address: String,
}

impl FlashbotsSigner {
    /// Create a new signer from a hex-encoded private key
    /// 
    /// # Arguments
    /// 
    /// * `private_key_hex` - Hex-encoded private key (with or without 0x prefix)
    /// 
    /// # Returns
    /// 
    /// A configured signer ready to authenticate requests
    pub fn new(private_key_hex: &str) -> Result<Self, AuthError> {
        let secp = Secp256k1::new();
        
        // Remove 0x prefix if present
        let key_hex = private_key_hex.trim_start_matches("0x");
        
        // Parse the private key
        let key_bytes = hex::decode(key_hex)
            .map_err(|e| AuthError::InvalidKey(format!("Invalid hex: {}", e)))?;
            
        let secret_key = SecretKey::from_slice(&key_bytes)?;
        
        // Derive the public key and address
        let public_key = secret_key.public_key(&secp);
        let public_key_bytes = public_key.serialize_uncompressed();
        
        // Ethereum address is last 20 bytes of keccak256 hash of public key (excluding prefix)
        let mut hasher = Keccak256::new();
        hasher.update(&public_key_bytes[1..]); // Skip the 0x04 prefix
        let hash = hasher.finalize();
        let address = format!("0x{}", hex::encode(&hash[12..]));
        
        Ok(Self {
            secp,
            secret_key,
            address,
        })
    }
    
    /// Get the Ethereum address associated with this signer
    /// 
    /// This is the address that will be included in the authentication header
    pub fn address(&self) -> &str {
        &self.address
    }
    
    /// Sign a request body using EIP-191
    /// 
    /// # Arguments
    /// 
    /// * `body` - The JSON request body to sign
    /// 
    /// # Returns
    /// 
    /// A signature string in the format "0xaddress:0xsignature"
    pub fn sign_request(&self, body: &str) -> Result<String, AuthError> {
        // Create EIP-191 message
        let message_to_sign = self.create_eip191_message(body);
        
        // Hash the message
        let mut hasher = Keccak256::new();
        hasher.update(&message_to_sign);
        let hash = hasher.finalize();
        
        // Create secp256k1 message
        let message = Message::from_slice(&hash)?;
        
        // Sign the message
        let signature = self.secp.sign_ecdsa(&message, &self.secret_key);
        let sig_bytes = signature.serialize_compact();
        
        // Format as "address:signature"
        Ok(format!("{}:0x{}", self.address, hex::encode(sig_bytes)))
    }
    
    /// Create an EIP-191 compliant message
    /// 
    /// The format is: "\x19Ethereum Signed Message:\n" + len(message) + message
    fn create_eip191_message(&self, body: &str) -> Vec<u8> {
        let prefix = "\x19Ethereum Signed Message:\n";
        let body_bytes = body.as_bytes();
        let len_str = body_bytes.len().to_string();
        
        let mut message = Vec::new();
        message.extend_from_slice(prefix.as_bytes());
        message.extend_from_slice(len_str.as_bytes());
        message.extend_from_slice(body_bytes);
        
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_signer_creation() {
        // Test private key (DO NOT USE IN PRODUCTION)
        let private_key = "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        
        let signer = FlashbotsSigner::new(private_key).unwrap();
        assert!(!signer.address().is_empty());
        assert!(signer.address().starts_with("0x"));
        assert_eq!(signer.address().len(), 42); // 0x + 40 hex chars
    }
    
    #[test]
    fn test_signing() {
        let private_key = "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let signer = FlashbotsSigner::new(private_key).unwrap();
        
        let body = r#"{"jsonrpc":"2.0","method":"eth_sendBundle","params":[],"id":1}"#;
        let signature = signer.sign_request(body).unwrap();
        
        // Check signature format
        assert!(signature.contains(':'));
        let parts: Vec<&str> = signature.split(':').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], signer.address());
        assert!(parts[1].starts_with("0x"));
    }
}