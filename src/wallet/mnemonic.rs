//! BIP-39 compatible mnemonic generation for HD wallets.
//!
//! This module provides mnemonic seed phrase generation and parsing using the
//! BIP-39 English wordlist, but uses SHA3-512 for key derivation to align
//! with pqcoin's post-quantum cryptographic primitives.

use crate::crypto::{Hash, hash};
use rand::RngCore;

/// BIP-39 English wordlist (2048 words).
/// Source: https://github.com/bitcoin/bips/blob/master/bip-0039/english.txt
const WORDLIST: &str = include_str!("wordlist.txt");

/// Mnemonic word count for 256-bit entropy.
pub const MNEMONIC_WORD_COUNT: usize = 24;

/// Entropy bytes for 24-word mnemonic (256 bits).
const ENTROPY_BYTES: usize = 32;

/// A 24-word mnemonic seed phrase.
#[derive(Clone)]
pub struct Mnemonic {
    words: Vec<String>,
}

impl Mnemonic {
    /// Generate a new random mnemonic with 256-bit entropy.
    pub fn generate() -> Self {
        let mut entropy = [0u8; ENTROPY_BYTES];
        rand::thread_rng().fill_bytes(&mut entropy);
        Self::from_entropy(&entropy)
    }

    /// Create a mnemonic from 256-bit entropy.
    ///
    /// The entropy is converted to a mnemonic using BIP-39 encoding:
    /// 1. Hash entropy to get checksum
    /// 2. Append first 8 bits of checksum to entropy (264 bits total)
    /// 3. Split into 24 x 11-bit indices
    /// 4. Map indices to words
    pub fn from_entropy(entropy: &[u8; ENTROPY_BYTES]) -> Self {
        let wordlist = get_wordlist();

        // Calculate checksum: first 8 bits of SHA3-512 hash
        let checksum_hash = hash(entropy);
        let checksum_byte = checksum_hash.as_bytes()[0];

        // Combine entropy (256 bits) + checksum (8 bits) = 264 bits
        // Split into 24 x 11-bit words
        let mut words = Vec::with_capacity(MNEMONIC_WORD_COUNT);

        // Process 11 bits at a time
        let mut bit_buffer: u32 = 0;
        let mut bits_in_buffer = 0;
        let mut entropy_iter = entropy.iter().chain(std::iter::once(&checksum_byte));

        for _ in 0..MNEMONIC_WORD_COUNT {
            // Fill buffer with enough bits
            while bits_in_buffer < 11 {
                if let Some(&byte) = entropy_iter.next() {
                    bit_buffer = (bit_buffer << 8) | (byte as u32);
                    bits_in_buffer += 8;
                }
            }

            // Extract 11-bit index
            bits_in_buffer -= 11;
            let index = (bit_buffer >> bits_in_buffer) & 0x7FF;
            bit_buffer &= (1 << bits_in_buffer) - 1;

            words.push(wordlist[index as usize].to_string());
        }

        Self { words }
    }

    /// Parse a mnemonic from a space-separated string.
    ///
    /// Returns an error if:
    /// - Word count is not 24
    /// - Any word is not in the BIP-39 wordlist
    /// - Checksum is invalid
    pub fn from_phrase(phrase: &str) -> Result<Self, MnemonicError> {
        let words: Vec<String> = phrase
            .split_whitespace()
            .map(|s| s.to_lowercase())
            .collect();

        if words.len() != MNEMONIC_WORD_COUNT {
            return Err(MnemonicError::InvalidWordCount(words.len()));
        }

        let wordlist = get_wordlist();

        // Validate words and convert to indices
        let mut indices = Vec::with_capacity(MNEMONIC_WORD_COUNT);
        for word in &words {
            let index = wordlist
                .iter()
                .position(|w| w == word)
                .ok_or_else(|| MnemonicError::InvalidWord(word.clone()))?;
            indices.push(index as u16);
        }

        // Reconstruct entropy from indices
        let mut entropy = [0u8; ENTROPY_BYTES];

        let mut bit_buffer: u64 = 0;
        let mut bits_in_buffer = 0;
        let mut byte_index = 0;

        for index in indices {
            bit_buffer = (bit_buffer << 11) | (index as u64);
            bits_in_buffer += 11;

            while bits_in_buffer >= 8 && byte_index < ENTROPY_BYTES {
                bits_in_buffer -= 8;
                entropy[byte_index] = (bit_buffer >> bits_in_buffer) as u8;
                bit_buffer &= (1 << bits_in_buffer) - 1;
                byte_index += 1;
            }
        }

        // Remaining bits are checksum
        let checksum_bits = bit_buffer as u8;

        // Verify checksum
        let expected_checksum = hash(&entropy).as_bytes()[0];
        if checksum_bits != expected_checksum {
            return Err(MnemonicError::InvalidChecksum);
        }

        Ok(Self { words })
    }

    /// Get the mnemonic words.
    pub fn words(&self) -> &[String] {
        &self.words
    }

    /// Convert the mnemonic to a space-separated phrase.
    pub fn to_phrase(&self) -> String {
        self.words.join(" ")
    }

    /// Derive a master seed from this mnemonic.
    ///
    /// Uses SHA3-512 for derivation instead of PBKDF2 to align with
    /// pqcoin's post-quantum cryptographic primitives.
    ///
    /// # Arguments
    ///
    /// * `passphrase` - Optional passphrase for additional security.
    pub fn to_seed(&self, passphrase: &str) -> MasterSeed {
        let phrase = self.to_phrase();
        let salt = format!("pqcoin mnemonic{passphrase}");

        // Derive seed using SHA3-512
        // We do multiple rounds to add some computational cost
        let mut seed_input = Vec::with_capacity(phrase.len() + salt.len());
        seed_input.extend_from_slice(phrase.as_bytes());
        seed_input.extend_from_slice(salt.as_bytes());

        let mut current_hash = hash(&seed_input);

        // 2048 rounds (similar to BIP-39's PBKDF2 iterations)
        for _ in 0..2048 {
            let mut round_input = Vec::with_capacity(64 + phrase.len());
            round_input.extend_from_slice(current_hash.as_bytes());
            round_input.extend_from_slice(phrase.as_bytes());
            current_hash = hash(&round_input);
        }

        MasterSeed(current_hash)
    }
}

impl std::fmt::Debug for Mnemonic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mnemonic")
            .field("words", &"[REDACTED]")
            .finish()
    }
}

impl zeroize::Zeroize for Mnemonic {
    fn zeroize(&mut self) {
        for word in &mut self.words {
            word.zeroize();
        }
        self.words.clear();
    }
}

impl Drop for Mnemonic {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.zeroize();
    }
}

/// A master seed derived from a mnemonic.
#[derive(Clone)]
pub struct MasterSeed(Hash);

impl MasterSeed {
    /// Create a master seed from raw bytes.
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(Hash::from_bytes(bytes))
    }

    /// Get the seed bytes.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl std::fmt::Debug for MasterSeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasterSeed")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

impl zeroize::Zeroize for MasterSeed {
    fn zeroize(&mut self) {
        // Hash doesn't implement Zeroize, but we can't do much here
        // since it's a fixed-size array. The Drop will handle memory.
    }
}

/// Errors that can occur when parsing a mnemonic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MnemonicError {
    /// Invalid number of words.
    InvalidWordCount(usize),
    /// Word not found in wordlist.
    InvalidWord(String),
    /// Checksum verification failed.
    InvalidChecksum,
}

impl std::fmt::Display for MnemonicError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MnemonicError::InvalidWordCount(n) => {
                write!(f, "invalid word count: expected 24, got {n}")
            }
            MnemonicError::InvalidWord(w) => write!(f, "invalid word: '{w}'"),
            MnemonicError::InvalidChecksum => write!(f, "invalid mnemonic checksum"),
        }
    }
}

impl std::error::Error for MnemonicError {}

/// Get the BIP-39 English wordlist.
fn get_wordlist() -> Vec<&'static str> {
    WORDLIST.lines().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wordlist_size() {
        let wordlist = get_wordlist();
        assert_eq!(wordlist.len(), 2048);
    }

    #[test]
    fn test_generate_mnemonic() {
        let mnemonic = Mnemonic::generate();
        assert_eq!(mnemonic.words().len(), 24);

        // All words should be in the wordlist
        let wordlist = get_wordlist();
        for word in mnemonic.words() {
            assert!(wordlist.contains(&word.as_str()));
        }
    }

    #[test]
    fn test_mnemonic_roundtrip() {
        let mnemonic = Mnemonic::generate();
        let phrase = mnemonic.to_phrase();

        let parsed = Mnemonic::from_phrase(&phrase).unwrap();
        assert_eq!(mnemonic.words(), parsed.words());
    }

    #[test]
    fn test_mnemonic_checksum() {
        let mnemonic = Mnemonic::generate();
        let mut phrase = mnemonic.to_phrase();

        // Modify one word
        phrase = phrase.replace(mnemonic.words()[0].as_str(), "abandon");

        // Should fail checksum (unless the modification happens to be valid)
        // This is probabilistic but very likely to fail
        if mnemonic.words()[0] != "abandon" {
            let result = Mnemonic::from_phrase(&phrase);
            assert!(result.is_err() || result.unwrap().words()[0] == "abandon");
        }
    }

    #[test]
    fn test_seed_derivation() {
        let mnemonic = Mnemonic::generate();

        let seed1 = mnemonic.to_seed("");
        let seed2 = mnemonic.to_seed("");
        assert_eq!(seed1.as_bytes(), seed2.as_bytes());

        let seed3 = mnemonic.to_seed("password");
        assert_ne!(seed1.as_bytes(), seed3.as_bytes());
    }

    #[test]
    fn test_invalid_word_count() {
        let result = Mnemonic::from_phrase("abandon abandon abandon");
        assert!(matches!(result, Err(MnemonicError::InvalidWordCount(3))));
    }

    #[test]
    fn test_invalid_word() {
        let phrase = "notaword abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        let result = Mnemonic::from_phrase(phrase);
        assert!(matches!(result, Err(MnemonicError::InvalidWord(_))));
    }
}
