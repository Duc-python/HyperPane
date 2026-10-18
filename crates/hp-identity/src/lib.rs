//! Device identity: who this machine is, and how another one can be sure.
//!
//! Two machines that have never met must be able to recognise each other later,
//! and the recognition has to survive everything between them: no server, no
//! account, no certificate authority. So the identity is a key pair, and the
//! name of a device *is* its public key. Nothing has to be looked up, nothing
//! can be renamed behind your back, and a machine that shows up with a different
//! key for a name you already know is a fact rather than an accusation.
//!
//! Pairing is the one moment a person carries a secret from one machine to the
//! other: a code, shown on one screen and typed on the other. Two things are
//! worth knowing about it, and both are in the code below:
//!
//! - **The code is long on purpose.** Anyone who records a pairing handshake can
//!   test guesses against it offline, so a six-digit code is not a secret, it is
//!   a speed bump. Twelve symbols of a thirty-two symbol alphabet is sixty bits,
//!   and it is stretched before use, which is what makes the offline search
//!   hopeless rather than quick.
//! - **Signing is not hand-rolled.** ed25519 and the hashes come from the
//!   RustCrypto family. This crate decides *what* is signed and *when* something
//!   is trusted; the arithmetic is somebody else's, reviewed problem.

pub mod pairing;
pub mod peers;

pub use pairing::{Answer, Confirmed, Offer, PairingError};
pub use peers::{Peer, Peers, PeersError, Remembered};

use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// How many times a pairing code is stretched before it becomes a key.
///
/// The number is the point: a guess costs this many hashes, so sixty bits of
/// code stops being reachable even with a recorded handshake. It is the
/// standard-sized cost rather than a clever one: what matters is that it is
/// paid, and that it is written down here rather than in a comment.
pub const CODE_STRETCH: u32 = 600_000;

/// No lookalikes: a code is read off one screen and typed into another, and `0`
/// against `O` is a person's mistake, not an attacker's opportunity.
const CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKLMNPQRSTUVWXYZ";

/// Symbols in a code, and the bits that buys: 32 symbols per symbol, so five
/// bits each, so sixty bits in total.
const CODE_LENGTH: usize = 12;

/// The version of the key file this build writes.
const KEY_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("the key file could not be read or written: {0}")]
    Io(#[from] std::io::Error),
    #[error("the key file is not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("the key file is version {found}; this build understands version {expected}")]
    Version { found: u32, expected: u32 },
    #[error("the key file does not hold a usable key")]
    BadSecret,
    #[error("{0} is not a device id: it should be sixty-four hexadecimal characters")]
    NotADeviceId(String),
    #[error("{0} is not a pairing code: twelve symbols, and only from the code alphabet")]
    NotAPairingCode(String),
}

/// A device's public key, in the form a person and a file can both hold.
///
/// It is the whole key rather than a hash of it: an id that hashes the key would
/// need the key to be found somewhere before it could verify anything, which is
/// exactly the lookup a key pair is being used to avoid.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(String);

impl DeviceId {
    /// Read an id from what a person pasted, or from a file.
    pub fn parse(text: &str) -> Result<Self, IdentityError> {
        let cleaned: String = text
            .chars()
            .filter(|character| !character.is_whitespace() && *character != '-')
            .collect::<String>()
            .to_lowercase();

        if cleaned.len() != 64
            || !cleaned
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err(IdentityError::NotADeviceId(text.to_string()));
        }

        Ok(Self(cleaned))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The first eight characters. What a person reads out or compares by eye;
    /// the whole id is what is actually pinned.
    pub fn short(&self) -> &str {
        &self.0[..8]
    }

    /// The whole id in groups, for putting two machines side by side.
    pub fn fingerprint(&self) -> String {
        self.0
            .as_bytes()
            .chunks(4)
            .map(|group| String::from_utf8_lossy(group).to_string())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn key(&self) -> Option<VerifyingKey> {
        let mut bytes = [0u8; 32];
        for (index, byte) in self.0.as_bytes().chunks(2).enumerate() {
            let pair = std::str::from_utf8(byte).ok()?;
            bytes[index] = u8::from_str_radix(pair, 16).ok()?;
        }
        VerifyingKey::from_bytes(&bytes).ok()
    }

    /// Whether this device signed this message.
    ///
    /// Nothing else is needed to check it: the id is the key.
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        let Some(key) = self.key() else {
            return false;
        };
        let Ok(signature) = Signature::from_slice(signature) else {
            return false;
        };
        key.verify_strict(message, &signature).is_ok()
    }
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// This machine's key pair.
pub struct Identity {
    signing: SigningKey,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyFile {
    version: u32,
    /// The private key, hex. It is a secret and is treated as one, but it is not
    /// encrypted: what would encrypt it is a password to type at every start,
    /// and the honest protection for a file like this is the account it lives in.
    secret: String,
}

impl Identity {
    /// A new device identity.
    pub fn generate() -> Self {
        Self {
            signing: SigningKey::generate(&mut OsRng),
        }
    }

    /// How this device is named, to itself and to everyone else.
    pub fn id(&self) -> DeviceId {
        DeviceId(hex(&self.signing.verifying_key().to_bytes()))
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.signing.sign(message).to_bytes().to_vec()
    }

    /// Read a key from a file, or say why not.
    pub fn load(path: &Path) -> Result<Self, IdentityError> {
        let text = std::fs::read_to_string(path)?;
        Self::from_json(&text)
    }

    pub fn from_json(text: &str) -> Result<Self, IdentityError> {
        let file: KeyFile = serde_json::from_str(text)?;
        if file.version != KEY_VERSION {
            return Err(IdentityError::Version {
                found: file.version,
                expected: KEY_VERSION,
            });
        }

        let bytes = unhex(&file.secret).ok_or(IdentityError::BadSecret)?;
        let secret: [u8; 32] = bytes.try_into().map_err(|_| IdentityError::BadSecret)?;

        Ok(Self {
            signing: SigningKey::from_bytes(&secret),
        })
    }

    /// Write the key where the app keeps its own files.
    ///
    /// On Unix the file is made readable only by its owner. On Windows there is
    /// no equivalent in the standard library - it takes an ACL - so the honest
    /// statement is that the protection is the account and its profile
    /// directory, and that a machine with another administrator is not protected
    /// from itself.
    pub fn save(&self, path: &Path) -> Result<(), IdentityError> {
        let file = KeyFile {
            version: KEY_VERSION,
            secret: hex(&self.signing.to_bytes()),
        };
        let text = serde_json::to_string_pretty(&file)?;
        std::fs::write(path, text)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }

    /// The identity in a file, made once and kept.
    pub fn load_or_create(path: &Path) -> Result<Self, IdentityError> {
        match Self::load(path) {
            Ok(identity) => Ok(identity),
            Err(IdentityError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                let identity = Self::generate();
                identity.save(path)?;
                Ok(identity)
            }
            Err(error) => Err(error),
        }
    }
}

impl std::fmt::Debug for Identity {
    /// Printed by what it is, never by its secret.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Identity")
            .field("id", &self.id().short())
            .finish()
    }
}

/// The code a person carries from one machine to the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCode(String);

/// The key a pairing settled on, derived once from the code and both nonces.
///
/// A pairing proves itself twice, once each way, and deriving the key again for
/// the second proof would pay the stretch twice for no extra safety. The bytes
/// are private and the type prints as its name, so a key cannot leak through a
/// log by accident.
pub struct PairingKey([u8; 32]);

impl std::fmt::Debug for PairingKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PairingKey")
    }
}

impl PairingKey {
    /// A proof over one transcript, under this key.
    pub fn proof(&self, transcript: &[u8]) -> [u8; 32] {
        let mut mac =
            <Hmac<Sha256> as Mac>::new_from_slice(&self.0).expect("hmac takes a key of any length");
        mac.update(transcript);
        mac.finalize().into_bytes().into()
    }

    /// Whether a proof fits this transcript, compared in constant time.
    pub fn matches(&self, transcript: &[u8], proof: &[u8]) -> bool {
        let mut mac =
            <Hmac<Sha256> as Mac>::new_from_slice(&self.0).expect("hmac takes a key of any length");
        mac.update(transcript);
        mac.verify_slice(proof).is_ok()
    }
}

impl PairingCode {
    /// The key for one handshake: the one place the stretch is paid.
    ///
    /// The salt is both machines' nonces, so the same code yields a different key
    /// for every pairing and a recording of one cannot be replayed into another.
    pub fn key(&self, salt: &[u8]) -> PairingKey {
        PairingKey(self.key_with(CODE_STRETCH, salt))
    }

    /// A fresh code. Sixty bits of it, from the operating system's randomness.
    pub fn generate() -> Self {
        let mut bytes = [0u8; CODE_LENGTH];
        OsRng.fill_bytes(&mut bytes);
        // The alphabet is thirty-two symbols and a byte has two hundred and
        // fifty-six values, so `% 32` is exact and every symbol is equally
        // likely: no modulo bias to think about.
        let code: String = bytes
            .iter()
            .map(|byte| CODE_ALPHABET[*byte as usize % CODE_ALPHABET.len()] as char)
            .collect();
        Self(code)
    }

    /// Read a code from what a person typed: case, spaces and dashes forgiven.
    pub fn parse(text: &str) -> Result<Self, IdentityError> {
        let cleaned: String = text
            .chars()
            .filter(|character| !character.is_whitespace() && *character != '-')
            .flat_map(|character| character.to_uppercase())
            .collect();

        if cleaned.len() != CODE_LENGTH
            || !cleaned.bytes().all(|byte| CODE_ALPHABET.contains(&byte))
        {
            return Err(IdentityError::NotAPairingCode(text.to_string()));
        }

        Ok(Self(cleaned))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The same, with the cost chosen by the caller.
    ///
    /// The number of rounds is policy rather than secret, and lowering it is
    /// what a test that only checks the plumbing should do. `key` is what a
    /// pairing calls.
    pub fn key_with(&self, rounds: u32, salt: &[u8]) -> [u8; 32] {
        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(self.0.as_bytes(), salt, rounds, &mut key);
        key
    }

    /// What both sides send to prove they know the code, over everything said so
    /// far: swap a message and the proof does not fit it any more.
    pub fn confirm(&self, salt: &[u8], transcript: &[u8]) -> [u8; 32] {
        self.key(salt).proof(transcript)
    }

    pub fn confirm_with(&self, rounds: u32, salt: &[u8], transcript: &[u8]) -> [u8; 32] {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.key_with(rounds, salt))
            .expect("hmac takes a key of any length");
        mac.update(transcript);
        mac.finalize().into_bytes().into()
    }

    /// Whether the other side knows the same code.
    pub fn confirms(&self, salt: &[u8], transcript: &[u8], proof: &[u8]) -> bool {
        self.key(salt).matches(transcript, proof)
    }

    pub fn confirms_with(&self, rounds: u32, salt: &[u8], transcript: &[u8], proof: &[u8]) -> bool {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.key_with(rounds, salt))
            .expect("hmac takes a key of any length");
        mac.update(transcript);
        mac.verify_slice(proof).is_ok()
    }
}

impl std::fmt::Display for PairingCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // In fours: a code is read aloud, and groups are how that is done.
        let grouped: Vec<String> = self
            .0
            .as_bytes()
            .chunks(4)
            .map(|group| String::from_utf8_lossy(group).to_string())
            .collect();
        formatter.write_str(&grouped.join("-"))
    }
}

/// A nonce, for a challenge or for a handshake's salt.
pub fn nonce(bytes: usize) -> Vec<u8> {
    let mut buffer = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buffer);
    buffer
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.len() % 2 != 0 {
        return None;
    }
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(pair, 16).ok()
        })
        .collect()
}

/// A digest, for anywhere this crate or its callers need a short commitment to
/// something longer.
pub fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_identity_signs_and_its_id_is_all_that_is_needed_to_check_it() {
        let identity = Identity::generate();
        let message = b"a handshake, or a dated note";

        let signature = identity.sign(message);
        assert_eq!(signature.len(), 64);

        // The id alone verifies: no lookup, no registry, no key exchange.
        let id = DeviceId::parse(identity.id().as_str()).expect("its own id");
        assert!(id.verify(message, &signature));
    }

    #[test]
    fn a_changed_message_or_a_changed_key_does_not_verify() {
        let identity = Identity::generate();
        let signature = identity.sign(b"the original");

        assert!(!identity.id().verify(b"something else", &signature));

        // Another device cannot pass for this one, which is the whole point.
        let other = Identity::generate();
        assert!(!other.id().verify(b"the original", &signature));
        assert_ne!(identity.id(), other.id());
    }

    #[test]
    fn a_signature_that_is_not_a_signature_is_refused_rather_than_panicking() {
        let identity = Identity::generate();
        assert!(!identity.id().verify(b"anything", b"too short"));
        assert!(!identity.id().verify(b"anything", &[]));
    }

    #[test]
    fn an_id_is_readable_and_comparable() {
        let identity = Identity::generate();
        let id = identity.id();

        assert_eq!(id.as_str().len(), 64);
        assert!(id.as_str().chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(id.short().len(), 8);
        assert!(id.as_str().starts_with(id.short()));

        // Groups of four, because that is how a person compares two of them.
        let fingerprint = id.fingerprint();
        let groups: Vec<&str> = fingerprint.split(' ').collect();
        assert_eq!(groups.len(), 16);
        assert!(groups.iter().all(|group| group.len() == 4));
    }

    #[test]
    fn an_id_given_with_spaces_or_capitals_is_still_that_id() {
        let identity = Identity::generate();
        let written = identity.id().fingerprint().to_uppercase();

        assert_eq!(DeviceId::parse(&written).expect("it parses"), identity.id());
    }

    #[test]
    fn something_that_is_not_an_id_says_so_instead_of_being_used() {
        let too_long = "z".repeat(64);
        let too_short = "a".repeat(63);

        for text in ["", "abc", too_long.as_str(), too_short.as_str()] {
            assert!(
                matches!(DeviceId::parse(text), Err(IdentityError::NotADeviceId(_))),
                "{text:?} was accepted as a device id"
            );
        }
    }

    #[test]
    fn a_key_round_trips_through_a_file_and_is_never_regenerated() {
        let directory = std::env::temp_dir().join(format!("hp-identity-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a directory");
        let path = directory.join("device.json");

        let first = Identity::load_or_create(&path).expect("a new identity");
        let again = Identity::load_or_create(&path).expect("the same identity");

        assert_eq!(first.id(), again.id());
        // And it is really the same key, not merely the same id.
        let signature = first.sign(b"proof");
        assert!(again.id().verify(b"proof", &signature));

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_key_file_that_is_not_one_is_refused_with_a_reason() {
        assert!(matches!(
            Identity::from_json("not json at all"),
            Err(IdentityError::Parse(_))
        ));
        assert!(matches!(
            Identity::from_json(r#"{"version": 9, "secret": "00"}"#),
            Err(IdentityError::Version { found: 9, .. })
        ));
        assert!(matches!(
            Identity::from_json(r#"{"version": 1, "secret": "not hex"}"#),
            Err(IdentityError::BadSecret)
        ));
        assert!(matches!(
            Identity::from_json(r#"{"version": 1, "secret": "abcd"}"#),
            Err(IdentityError::BadSecret)
        ));
    }

    #[test]
    fn a_generated_code_is_the_length_and_alphabet_it_claims() {
        let code = PairingCode::generate();
        assert_eq!(code.as_str().len(), CODE_LENGTH);
        assert!(code
            .as_str()
            .bytes()
            .all(|byte| CODE_ALPHABET.contains(&byte)));

        // Two codes are not the same code, which is the least randomness can do.
        assert_ne!(code, PairingCode::generate());
    }

    #[test]
    fn a_code_is_read_the_way_a_person_writes_it_down() {
        let code = PairingCode::generate();
        let written = code.to_string().to_lowercase();

        assert_eq!(PairingCode::parse(&written).expect("it parses"), code);
        // Dashes and spaces are how it is displayed, not part of it.
        assert!(PairingCode::parse(&format!(" {written} ")).is_ok());
    }

    #[test]
    fn a_code_that_could_not_have_been_generated_is_refused() {
        for text in ["", "SHORT", "IIIIIIIIIIII", "AAAAAAAAAAAA!", "ABCDEFGHIJK"] {
            assert!(
                matches!(
                    PairingCode::parse(text),
                    Err(IdentityError::NotAPairingCode(_))
                ),
                "{text:?} was accepted as a pairing code"
            );
        }
    }

    /// A round count for the plumbing tests.
    ///
    /// The cost is what `key` pays, and paying it in every case would make the
    /// suite slow enough that people stop running it. One test pays it for real.
    const PLAIN: u32 = 1_000;

    #[test]
    fn both_sides_of_a_pairing_agree_when_they_have_the_same_code() {
        let code = PairingCode::generate();
        let salt = nonce(32);
        let transcript = b"the handshake, as both sides saw it";

        let theirs = code.confirm_with(PLAIN, &salt, transcript);
        assert!(code.confirms_with(PLAIN, &salt, transcript, &theirs));
    }

    #[test]
    fn a_pairing_pays_the_real_cost_and_still_agrees() {
        // The number in `CODE_STRETCH` is exercised here rather than assumed.
        let code = PairingCode::generate();
        let salt = nonce(32);

        let proof = code.confirm(&salt, b"a real pairing");
        assert!(code.confirms(&salt, b"a real pairing", &proof));
    }

    #[test]
    fn a_different_code_or_a_changed_handshake_is_not_agreed() {
        let code = PairingCode::generate();
        let salt = nonce(32);
        let transcript = b"the handshake";
        let proof = code.confirm_with(PLAIN, &salt, transcript);

        // Someone guessing a code, or someone in the middle changing what was
        // said: neither can produce the same proof.
        let guess = PairingCode::generate();
        assert!(!guess.confirms_with(PLAIN, &salt, transcript, &proof));
        assert!(!code.confirms_with(PLAIN, &salt, b"a different handshake", &proof));
        assert!(!code.confirms_with(PLAIN, &nonce(32), transcript, &proof));
        assert!(!code.confirms_with(PLAIN, &salt, transcript, b"a shorter proof"));
    }

    #[test]
    fn the_same_code_gives_a_different_key_for_every_handshake() {
        let code = PairingCode::generate();
        let salt = nonce(32);
        let other = nonce(32);

        // A recording of one pairing is not a key for the next one, because the
        // salt is that handshake's own nonces.
        assert_ne!(code.key_with(PLAIN, &salt), code.key_with(PLAIN, &other));
        // And it is deterministic for one salt, which is what lets both sides
        // arrive at the same key without sending it.
        assert_eq!(code.key_with(PLAIN, &salt), code.key_with(PLAIN, &salt));
    }
}
