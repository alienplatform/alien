//! The recovery code and the key it derives.
//!
//! The code is 128 random bits, shown once to the first admin as 32
//! characters: the bits and a 32-bit check, in Crockford base32. The key is
//! derived from it with HKDF-SHA256 to 48 bytes and reduced into the P-256
//! scalar range (FIPS 186-5 A.2.1), so the same code always gives the same
//! key and the code is never stored anywhere.

use std::fmt;

use alien_error::AlienError;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};
use crate::key::SecretKey;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const SECRET_BYTES: usize = 16;
const CHECK_BYTES: usize = 4;

/// P-256 group order minus one, big-endian.
const ORDER_MINUS_ONE: [u64; 4] = [
    0xFFFF_FFFF_0000_0000,
    0xFFFF_FFFF_FFFF_FFFF,
    0xBCE6_FAAD_A717_9E84,
    0xF3B9_CAC2_FC63_2550,
];

/// A recovery code.
#[derive(Clone, PartialEq, Eq)]
pub struct RecoveryCode([u8; SECRET_BYTES]);

impl fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryCode(..)")
    }
}

impl RecoveryCode {
    /// A new code from the operating system's generator.
    pub fn generate() -> Result<Self> {
        let mut secret = [0u8; SECRET_BYTES];
        getrandom::fill(&mut secret)
            .map_err(|error| invalid(format!("the system random source failed: {error}")))?;
        Ok(Self(secret))
    }

    /// Read a code as the admin typed it. Case, spaces and dashes don't
    /// matter, and the letters people confuse with digits read as those
    /// digits. A typo fails the check rather than deriving another key.
    pub fn parse(text: &str) -> Result<Self> {
        let mut bits: u128 = 0;
        let mut extra: u64 = 0;
        let mut count = 0usize;
        for character in text.chars() {
            if character == '-' || character.is_whitespace() {
                continue;
            }
            let value = decode_char(character)
                .ok_or_else(|| invalid(format!("'{character}' is not in a recovery code")))?;
            // The first 128 bits are the secret, the last 32 the check.
            if count < 25 {
                bits = (bits << 5) | u128::from(value);
            } else if count == 25 {
                bits = (bits << 3) | u128::from(value >> 2);
                extra = u64::from(value & 0b11);
            } else {
                extra = (extra << 5) | u64::from(value);
            }
            count += 1;
        }
        if count != 32 {
            return Err(invalid("a recovery code has 32 characters"));
        }
        let secret = bits.to_be_bytes();
        let code = Self(secret);
        if extra != u64::from(u32::from_be_bytes(code.check())) {
            return Err(invalid(
                "the recovery code doesn't check out; look for a mistyped character",
            ));
        }
        Ok(code)
    }

    /// The code in groups of four, as shown to the admin.
    pub fn display(&self) -> String {
        let mut bytes = [0u8; SECRET_BYTES + CHECK_BYTES];
        bytes[..SECRET_BYTES].copy_from_slice(&self.0);
        bytes[SECRET_BYTES..].copy_from_slice(&self.check());
        let characters = encode(&bytes);
        characters
            .as_bytes()
            .chunks(4)
            .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Whether `typed` is the last four characters of the code, read as
    /// [`parse`](Self::parse) reads them. Those characters are part of the
    /// check, not of the secret, so asking for them proves the admin saw the
    /// code without typing it back.
    pub fn ends_with(&self, typed: &str) -> bool {
        let typed: Vec<char> = typed
            .chars()
            .filter(|character| *character != '-' && !character.is_whitespace())
            .collect();
        if typed.len() != 4 {
            return false;
        }
        let shown = self.display().replace('-', "");
        let tail = &shown.as_bytes()[shown.len() - 4..];
        typed
            .iter()
            .zip(tail)
            .all(|(character, expected)| decode_char(*character) == decode_char(*expected as char))
    }

    /// The recovery key of `project`.
    pub fn key(&self, project: &str) -> Result<SecretKey> {
        let okm = hkdf_sha256(&self.0, format!("recovery\u{0}{project}").as_bytes())?;
        let mut scalar = reduce_mod_order_minus_one(&okm);
        add_one(&mut scalar);
        let mut bytes = [0u8; 32];
        for (index, limb) in scalar.iter().enumerate() {
            bytes[index * 8..index * 8 + 8].copy_from_slice(&limb.to_be_bytes());
        }
        SecretKey::from_secret_bytes(&bytes)
    }

    fn check(&self) -> [u8; CHECK_BYTES] {
        let digest = Sha256::digest(self.0);
        [digest[0], digest[1], digest[2], digest[3]]
    }
}

/// Crockford base32 of `bytes`, without padding. `bytes` is a multiple of 5.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(5) {
        let mut buffer = [0u8; 8];
        buffer[3..3 + chunk.len()].copy_from_slice(chunk);
        let value = u64::from_be_bytes(buffer);
        for index in (0..chunk.len() * 8 / 5).rev() {
            out.push(ALPHABET[((value >> (index * 5)) & 31) as usize] as char);
        }
    }
    out
}

fn decode_char(character: char) -> Option<u8> {
    let character = match character.to_ascii_uppercase() {
        'O' => '0',
        'I' | 'L' => '1',
        other => other,
    };
    ALPHABET
        .iter()
        .position(|&letter| letter as char == character)
        .map(|position| position as u8)
}

fn hkdf_sha256(ikm: &[u8], info: &[u8]) -> Result<[u8; 48]> {
    let mut extract =
        Hmac::<Sha256>::new_from_slice(&[0u8; 32]).map_err(|_| invalid("HKDF could not start"))?;
    extract.update(ikm);
    let prk = extract.finalize().into_bytes();
    let mut okm = [0u8; 48];
    let mut previous: Vec<u8> = Vec::new();
    for (counter, block) in okm.chunks_mut(32).enumerate() {
        let mut expand =
            Hmac::<Sha256>::new_from_slice(&prk).map_err(|_| invalid("HKDF could not expand"))?;
        expand.update(&previous);
        expand.update(info);
        expand.update(&[counter as u8 + 1]);
        let output = expand.finalize().into_bytes();
        block.copy_from_slice(&output[..block.len()]);
        previous = output.to_vec();
    }
    Ok(okm)
}

/// `value mod (n - 1)`, one bit at a time; the input is 384 bits.
fn reduce_mod_order_minus_one(value: &[u8; 48]) -> [u64; 4] {
    // Five limbs leave room for the shifted remainder before subtracting.
    let modulus = [
        0,
        ORDER_MINUS_ONE[0],
        ORDER_MINUS_ONE[1],
        ORDER_MINUS_ONE[2],
        ORDER_MINUS_ONE[3],
    ];
    let mut remainder = [0u64; 5];
    for byte in value {
        for bit in (0..8).rev() {
            let mut carry = u64::from((byte >> bit) & 1);
            for limb in remainder.iter_mut().rev() {
                let next = *limb >> 63;
                *limb = (*limb << 1) | carry;
                carry = next;
            }
            if remainder >= modulus {
                let mut borrow = 0u64;
                for index in (0..5).rev() {
                    let (difference, first) = remainder[index].overflowing_sub(modulus[index]);
                    let (difference, second) = difference.overflowing_sub(borrow);
                    remainder[index] = difference;
                    borrow = u64::from(first || second);
                }
            }
        }
    }
    [remainder[1], remainder[2], remainder[3], remainder[4]]
}

fn add_one(value: &mut [u64; 4]) {
    for limb in value.iter_mut().rev() {
        let (sum, overflow) = limb.overflowing_add(1);
        *limb = sum;
        if !overflow {
            return;
        }
    }
}

fn invalid(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::KeyInvalid {
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_reads_back_as_typed_and_derives_the_same_key() {
        let code = RecoveryCode::generate().unwrap();
        let shown = code.display();
        assert_eq!(shown.len(), 39, "{shown}");
        let typed = shown.to_lowercase().replace('-', " ");
        let read = RecoveryCode::parse(&typed).unwrap();
        assert_eq!(read, code);
        assert_eq!(
            read.key("prj_a").unwrap().public_key(),
            code.key("prj_a").unwrap().public_key()
        );
        assert_ne!(
            code.key("prj_a").unwrap().public_key(),
            code.key("prj_b").unwrap().public_key()
        );
    }

    #[test]
    fn the_last_four_characters_are_checked_as_typed() {
        let code = RecoveryCode([7u8; SECRET_BYTES]);
        let other = RecoveryCode([9u8; SECRET_BYTES]);
        let shown = code.display();
        let tail = &shown[shown.len() - 4..];
        let other_shown = other.display();
        assert_ne!(tail, &other_shown[other_shown.len() - 4..]);
        assert_ne!(tail, &shown[..4]);

        assert!(code.ends_with(tail));
        assert!(code.ends_with(&tail.to_lowercase()));
        assert!(code.ends_with(&format!("  {tail}\n")));
        assert!(!code.ends_with(""));
        assert!(!code.ends_with(&shown[shown.len() - 3..]));
        assert!(!code.ends_with(&shown[shown.len() - 6..]));
        assert!(!code.ends_with(&shown[..4]));
        assert!(!code.ends_with(&other_shown[other_shown.len() - 4..]));
    }

    #[test]
    fn a_mistyped_code_is_refused() {
        let shown = RecoveryCode::generate().unwrap().display();
        let mut characters: Vec<char> = shown.chars().collect();
        characters[0] = if characters[0] == 'A' { 'B' } else { 'A' };
        let typo: String = characters.into_iter().collect();
        RecoveryCode::parse(&typo).expect_err("typo");
        RecoveryCode::parse(&shown[..30]).expect_err("short");
    }

    #[test]
    fn hkdf_matches_rfc_5869_case_3() {
        // RFC 5869 A.3: zero-length salt and info, 42 bytes out. The salt of
        // HMAC-SHA256's block of zeros equals a zero-length salt.
        let ikm = [0x0bu8; 22];
        let okm = hkdf_sha256(&ikm, b"").unwrap();
        assert_eq!(
            hex::encode(&okm[..42]),
            "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d9d201395faa4b61a96c8"
        );
    }

    #[test]
    fn reduction_keeps_small_values_and_wraps_at_the_order() {
        let mut small = [0u8; 48];
        small[47] = 7;
        assert_eq!(reduce_mod_order_minus_one(&small), [0, 0, 0, 7]);
        let mut order_minus_one = [0u8; 48];
        for (index, limb) in ORDER_MINUS_ONE.iter().enumerate() {
            order_minus_one[16 + index * 8..24 + index * 8].copy_from_slice(&limb.to_be_bytes());
        }
        assert_eq!(reduce_mod_order_minus_one(&order_minus_one), [0, 0, 0, 0]);
        order_minus_one[47] += 1;
        assert_eq!(reduce_mod_order_minus_one(&order_minus_one), [0, 0, 0, 1]);
    }
}
