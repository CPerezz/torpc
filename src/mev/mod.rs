//! MEV (Maximum Extractable Value) protection module
//!
//! This module provides integration with Flashbots and other MEV relay services
//! to protect users from sandwich attacks and provide private transaction submission.
//!
//! # Overview
//!
//! MEV protection works by submitting transactions directly to block builders
//! through private relays instead of the public mempool. This prevents:
//! - Front-running attacks
//! - Sandwich attacks  
//! - Transaction censorship
//!
//! # For Proxy Operators
//!
//! To enable MEV protection, configure the following environment variables:
//! - `FLASHBOTS_RELAY_URL`: MEV relay endpoint (defaults to Flashbots mainnet)
//! - `FLASHBOTS_SIGNING_KEY`: Private key for authentication (any Ethereum key)
//!
//! # Architecture
//!
//! The module is organized as follows:
//! - `client` - Main MEV relay client with connection pooling
//! - `auth` - Flashbots authentication using EIP-191 signatures
//! - `types` - Data structures for bundles and responses
//! - `retry` - Fault tolerance with exponential backoff and circuit breakers

pub mod auth;
pub mod client;
pub mod mev_handler;
pub mod retry;
pub mod types;

// Re-export main types for convenience
pub use auth::{AuthError, FlashbotsAuthenticator};
pub use client::{create_mev_client, MevRelayClient};
pub use types::{Bundle, BundleResponse, MevConfig};
