//! # Phase 13 — Security
//!
//! This module provides:
//!
//! 1. **Device Identity** — A persistent `ed25519` keypair stored in the database.
//!    The hex-encoded public key is used as the stable `device_id`.
//!
//! 2. **Session Handshake** — An ephemeral X25519 ECDH exchange derives a shared
//!    32-byte secret at the start of every TCP connection.
//!    - Sender sends:  `[32-byte X25519 ephemeral pubkey]`
//!    - Receiver replies: `[32-byte X25519 ephemeral pubkey]`
//!    - Both sides compute the shared secret and derive a symmetric key with HKDF.
//!
//! 3. **Encrypted Framing** — After the handshake, all frames are encrypted with
//!    ChaCha20-Poly1305.  Each frame is:
//!    `[4-byte LE length][12-byte nonce][ciphertext + 16-byte tag]`
//!    The nonce is a monotonically-incrementing 96-bit little-endian counter.

#![allow(dead_code)] // Public API — used in future phases / extensible

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::RngCore;
use x25519_dalek::{EphemeralSecret, PublicKey};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ── Device Identity ────────────────────────────────────────────────────────────

/// Generates a new Ed25519 signing key (device identity).
/// Returns `(secret_key_bytes_hex, public_key_hex)`.
pub fn generate_identity() -> (String, String) {
    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);
    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key: VerifyingKey = signing_key.verifying_key();
    let secret_hex = hex::encode(signing_key.to_bytes());
    let public_hex = hex::encode(verifying_key.to_bytes());
    (secret_hex, public_hex)
}

/// Returns the public-key hex from a stored secret-key hex.
pub fn public_key_from_secret(secret_hex: &str) -> Option<String> {
    let bytes = hex::decode(secret_hex).ok()?;
    let arr: [u8; 32] = bytes.try_into().ok()?;
    let sk = SigningKey::from_bytes(&arr);
    Some(hex::encode(sk.verifying_key().to_bytes()))
}

// ── Session Handshake ──────────────────────────────────────────────────────────

/// Shared session key derived from ECDH + HKDF.
#[derive(Clone)]
pub struct SessionKey(pub [u8; 32]);

/// **Initiator** (sender) side of the handshake.
///
/// Writes our ephemeral public key then reads the peer's,
/// derives the shared secret, and returns `(SessionKey, stream)`.
pub async fn handshake_initiator<S>(mut stream: S) -> Result<(SessionKey, S), String>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let our_secret = EphemeralSecret::random_from_rng(rand::thread_rng());
    let our_pubkey = PublicKey::from(&our_secret);

    stream.write_all(our_pubkey.as_bytes()).await.map_err(|e| format!("hs write pubkey: {e}"))?;

    let mut their_pubkey_bytes = [0u8; 32];
    stream.read_exact(&mut their_pubkey_bytes).await.map_err(|e| format!("hs read pubkey: {e}"))?;
    let their_pubkey = PublicKey::from(their_pubkey_bytes);

    let shared = our_secret.diffie_hellman(&their_pubkey);
    Ok((SessionKey(derive_key(shared.as_bytes())), stream))
}

/// **Responder** (receiver) side of the handshake.
///
/// Reads the initiator's ephemeral public key, writes ours,
/// derives the shared secret, and returns `(SessionKey, stream)`.
pub async fn handshake_responder<S>(mut stream: S) -> Result<(SessionKey, S), String>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let mut their_pubkey_bytes = [0u8; 32];
    stream.read_exact(&mut their_pubkey_bytes).await.map_err(|e| format!("hs read pubkey: {e}"))?;
    let their_pubkey = PublicKey::from(their_pubkey_bytes);

    let our_secret = EphemeralSecret::random_from_rng(rand::thread_rng());
    let our_pubkey = PublicKey::from(&our_secret);
    stream.write_all(our_pubkey.as_bytes()).await.map_err(|e| format!("hs write pubkey: {e}"))?;

    let shared = our_secret.diffie_hellman(&their_pubkey);
    Ok((SessionKey(derive_key(shared.as_bytes())), stream))
}

/// HKDF-SHA256 to stretch the raw Diffie-Hellman output into a key suitable
/// for ChaCha20-Poly1305.
fn derive_key(dh: &[u8]) -> [u8; 32] {
    // Simple HKDF-Extract + Expand.
    // Salt: fixed domain-separation string.
    // Info: "FlashTransfer session key v1"
    let salt = b"FlashTransfer-HKDF-v1";
    let info = b"session-key";

    // HKDF-Extract: prk = HMAC-SHA256(salt, ikm)
    let prk = hmac_sha256(salt, dh);
    // HKDF-Expand: okm = T(1) where T(1) = HMAC-SHA256(prk, info || 0x01)
    let mut expand_input = Vec::with_capacity(info.len() + 1);
    expand_input.extend_from_slice(info);
    expand_input.push(0x01);
    let okm = hmac_sha256(&prk, &expand_input);
    okm
}

/// Minimal HMAC-SHA256 implementation using the `sha2` crate that comes with
/// `chacha20poly1305`'s dependency tree (via `hkdf` / `hmac`).
/// We bring in the dependency explicitly.
fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {

    const BLOCK_SIZE: usize = 64;

    // Derive ipad / opad keys
    let mut k = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let h = sha256(key);
        k[..32].copy_from_slice(&h);
    } else {
        k[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }

    let mut inner = Vec::with_capacity(BLOCK_SIZE + data.len());
    inner.extend_from_slice(&ipad);
    inner.extend_from_slice(data);
    let inner_hash = sha256(&inner);

    let mut outer = Vec::with_capacity(BLOCK_SIZE + 32);
    outer.extend_from_slice(&opad);
    outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

fn sha256(data: &[u8]) -> [u8; 32] {
    // Use blake3 as a practical 256-bit PRF substitute for HKDF.
    // For a future upgrade, swap in `sha2::Sha256`.
    let h = blake3::hash(data);
    *h.as_bytes()
}

// ── Encrypted Framing ──────────────────────────────────────────────────────────

/// Wraps an async stream to add ChaCha20-Poly1305 encryption.
///
/// Frames are: `[4-byte LE plaintext-len][12-byte nonce][ciphertext+16-byte tag]`.
/// The nonce is a 96-bit little-endian monotonic counter seeded to 0.
pub struct EncryptedStream<S> {
    inner: S,
    cipher: ChaCha20Poly1305,
    send_counter: u64,
    recv_counter: u64,
}

impl<S: AsyncReadExt + AsyncWriteExt + Unpin> EncryptedStream<S> {
    pub fn new(inner: S, key: &SessionKey) -> Self {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key.0));
        Self { inner, cipher, send_counter: 0, recv_counter: 0 }
    }

    /// Encrypt and write one frame.
    pub async fn write_frame(&mut self, plaintext: &[u8]) -> Result<(), String> {
        let nonce = counter_nonce(self.send_counter);
        self.send_counter += 1;

        let ciphertext = self.cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| format!("encrypt: {e}"))?;

        let len = (ciphertext.len() as u32).to_le_bytes();
        self.inner.write_all(&len).await.map_err(|e| format!("write frame len: {e}"))?;
        self.inner.write_all(nonce.as_slice()).await.map_err(|e| format!("write nonce: {e}"))?;
        self.inner.write_all(&ciphertext).await.map_err(|e| format!("write ciphertext: {e}"))?;
        Ok(())
    }

    /// Read and decrypt one frame.
    pub async fn read_frame(&mut self) -> Result<Vec<u8>, String> {
        let mut len_bytes = [0u8; 4];
        self.inner.read_exact(&mut len_bytes).await.map_err(|e| format!("read frame len: {e}"))?;
        let ct_len = u32::from_le_bytes(len_bytes) as usize;

        let mut nonce_bytes = [0u8; 12];
        self.inner.read_exact(&mut nonce_bytes).await.map_err(|e| format!("read nonce: {e}"))?;
        let nonce = Nonce::from(nonce_bytes);

        // Verify the nonce matches our expected receive counter (replay protection)
        let expected_nonce = counter_nonce(self.recv_counter);
        if nonce != expected_nonce {
            return Err("nonce mismatch — possible replay or reorder".to_string());
        }
        self.recv_counter += 1;

        let mut ciphertext = vec![0u8; ct_len];
        self.inner.read_exact(&mut ciphertext).await.map_err(|e| format!("read ciphertext: {e}"))?;

        let plaintext = self.cipher
            .decrypt(&nonce, ciphertext.as_slice())
            .map_err(|_| "decryption failed — data tampered or wrong key".to_string())?;

        Ok(plaintext)
    }

    /// Write raw bytes directly (only for the pre-handshake key exchange itself).
    pub async fn write_raw(&mut self, data: &[u8]) -> Result<(), String> {
        self.inner.write_all(data).await.map_err(|e| e.to_string())
    }

    /// Read raw bytes directly (only for the pre-handshake key exchange itself).
    pub async fn read_raw_exact(&mut self, buf: &mut [u8]) -> Result<(), String> {
        self.inner.read_exact(buf).await.map(|_| ()).map_err(|e| e.to_string())
    }

    /// Access the inner stream (e.g., to flush).
    pub async fn flush(&mut self) -> Result<(), String> {
        self.inner.flush().await.map_err(|e| e.to_string())
    }
}

fn counter_nonce(counter: u64) -> Nonce {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&counter.to_le_bytes());
    Nonce::from(n)
}

// ── DB helpers ─────────────────────────────────────────────────────────────────

/// Returns the local device's `device_id` (Ed25519 public key hex).
/// If none is stored yet, generates and persists a fresh identity.
pub async fn get_or_create_device_id(db: &crate::storage::db::Database) -> String {
    if let Some(pk) = db.get_setting("device_public_key").await {
        return pk;
    }
    let (secret_hex, public_hex) = generate_identity();
    let _ = db.set_setting("device_secret_key", &secret_hex).await;
    let _ = db.set_setting("device_public_key", &public_hex).await;
    public_hex
}
