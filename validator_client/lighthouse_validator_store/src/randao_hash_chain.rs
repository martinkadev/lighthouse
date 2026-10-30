//! Immutable, network- and validator-bound EIP-8321 chain custody.
//!
//! Store the seed durably before any registration can be signed. There is no mutable cursor:
//! canonical commitments select preimages, including after a restart or a reorg.
use bls::PublicKeyBytes;
use rand::TryRngCore;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};
use types::{Hash256, consts::heze::HASH_CHAIN_RANDAO_DST};
use zeroize::Zeroizing;

const CHAIN_LENGTH: u64 = 1 << 16;

#[derive(Serialize, Deserialize)]
struct StoredChain {
    version: u32,
    genesis_validators_root: Hash256,
    pubkey: PublicKeyBytes,
    validator_index: u64,
    length: u64,
    seed: Zeroizing<[u8; 32]>,
    tip: Hash256,
}

#[derive(Debug)]
pub struct PreparedChain {
    pub tip: Hash256,
    pub reveal: Option<Hash256>,
}

pub fn step(value: Hash256) -> Hash256 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(HASH_CHAIN_RANDAO_DST);
    hasher.update(value.as_slice());
    Hash256::from_slice(hasher.finalize().as_bytes())
}

impl StoredChain {
    fn walk(&self, commitment: Hash256) -> Result<PreparedChain, String> {
        if self.version != 1 || self.length == 0 || self.length > 1 << 24 {
            return Err("Unsupported RANDAO chain format or length".into());
        }
        let mut value = Hash256::from_slice(self.seed.as_ref());
        if value == Hash256::ZERO {
            return Err("Zero RANDAO chain seed".into());
        }
        let exhausted = value == commitment;
        let mut reveal = None;
        for _ in 0..self.length {
            let next = step(value);
            if next == Hash256::ZERO {
                return Err("Zero RANDAO chain link".into());
            }
            if next == commitment {
                reveal = Some(value);
            }
            value = next;
        }
        if value != self.tip {
            return Err("RANDAO chain integrity check failed".into());
        }
        if commitment != Hash256::ZERO && reveal.is_none() {
            return Err(if exhausted {
                "RANDAO hash chain exhausted"
            } else {
                "Canonical RANDAO commitment does not belong to the stored chain"
            }
            .into());
        }
        Ok(PreparedChain {
            tip: self.tip,
            reveal,
        })
    }
}

/// `allow_create` must be false when a canonical active or pending commitment exists.
pub fn prepare(
    directory: &Path,
    genesis: Hash256,
    pubkey: PublicKeyBytes,
    index: u64,
    commitment: Hash256,
    allow_create: bool,
) -> Result<PreparedChain, String> {
    let directory = directory.join(format!("{genesis:?}"));
    fs::create_dir_all(&directory).map_err(|e| format!("Create RANDAO custody directory: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("Protect RANDAO custody directory: {e}"))?;
    }
    // Sync the complete path even if another process just created it. Checking which
    // ancestors existed before create_dir_all would race another custody creator.
    for ancestor in directory
        .ancestors()
        .filter(|path| !path.as_os_str().is_empty())
    {
        File::open(ancestor)
            .and_then(|file| file.sync_all())
            .map_err(|e| format!("Sync RANDAO custody directory: {e}"))?;
    }
    let path = directory.join(format!("{pubkey}.json"));
    if !path
        .try_exists()
        .map_err(|e| format!("Inspect RANDAO custody file: {e}"))?
    {
        if !allow_create {
            return Err("Missing RANDAO chain; restore its backup before proposing".into());
        }
        let mut seed = Zeroizing::new([0u8; 32]);
        rand::rngs::OsRng
            .try_fill_bytes(seed.as_mut())
            .map_err(|e| format!("Generate RANDAO chain secret: {e}"))?;
        let mut tip = Hash256::from_slice(seed.as_ref());
        if tip == Hash256::ZERO {
            return Err("Random RANDAO seed was zero; retry generation".into());
        }
        for _ in 0..CHAIN_LENGTH {
            tip = step(tip);
            if tip == Hash256::ZERO {
                return Err("Random RANDAO chain contains zero; retry generation".into());
            }
        }
        let chain = StoredChain {
            version: 1,
            genesis_validators_root: genesis,
            pubkey,
            validator_index: index,
            length: CHAIN_LENGTH,
            seed,
            tip,
        };
        chain.walk(Hash256::ZERO)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&directory)
            .map_err(|e| format!("Create RANDAO custody file: {e}"))?;
        serde_json::to_writer(temporary.as_file_mut(), &chain)
            .map_err(|_| "Serialize RANDAO custody file".to_string())?;
        temporary
            .flush()
            .and_then(|()| temporary.as_file().sync_all())
            .map_err(|e| format!("Sync RANDAO custody file: {e}"))?;
        match temporary.persist_noclobber(&path) {
            Ok(_) => {}
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("Persist RANDAO custody file: {}", e.error)),
        }
        File::open(&directory)
            .and_then(|file| file.sync_all())
            .map_err(|e| format!("Sync RANDAO custody directory: {e}"))?;
    }
    let metadata =
        fs::symlink_metadata(&path).map_err(|e| format!("Inspect RANDAO custody file: {e}"))?;
    if !metadata.is_file() || metadata.len() > 4096 {
        return Err("RANDAO custody path must be a regular, bounded file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(
                "RANDAO custody file must not be accessible by group or other users".into(),
            );
        }
    }
    let mut bytes = Zeroizing::new(Vec::new());
    File::open(&path)
        .and_then(|file| file.take(4097).read_to_end(&mut bytes))
        .map_err(|e| format!("Read RANDAO custody file: {e}"))?;
    let chain: StoredChain = serde_json::from_slice(&bytes)
        .map_err(|_| "Invalid RANDAO custody file; restore its backup".to_string())?;
    if chain.genesis_validators_root != genesis
        || chain.pubkey != pubkey
        || chain.validator_index != index
    {
        return Err("RANDAO custody identity does not match the validator and network".into());
    }
    chain.walk(commitment)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key() -> PublicKeyBytes {
        PublicKeyBytes::empty()
    }
    #[test]
    fn chain_step_matches_local_python_spec() {
        let expected: Hash256 = "b16fcaaa6cf97c506a9ca4c4e04e766cec67d5e1bd308152be59d082bb52f187"
            .parse()
            .unwrap();
        assert_eq!(step(Hash256::repeat_byte(1)), expected);
    }

    #[test]
    fn restart_and_reorg_select_commitment_not_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let genesis = Hash256::repeat_byte(3);
        let original = prepare(dir.path(), genesis, key(), 7, Hash256::ZERO, true).unwrap();
        let first = prepare(dir.path(), genesis, key(), 7, original.tip, false)
            .unwrap()
            .reveal
            .unwrap();
        let second = prepare(dir.path(), genesis, key(), 7, first, false)
            .unwrap()
            .reveal
            .unwrap();
        assert_eq!(step(second), first);
        assert_eq!(step(first), original.tip);
        assert_eq!(
            prepare(dir.path(), genesis, key(), 7, original.tip, false)
                .unwrap()
                .reveal,
            Some(first)
        );
        assert_eq!(
            prepare(dir.path(), genesis, key(), 7, Hash256::ZERO, true)
                .unwrap()
                .tip,
            original.tip
        );
    }
    #[test]
    fn missing_unknown_and_wrong_identity_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        let genesis = Hash256::repeat_byte(3);
        assert!(
            prepare(
                dir.path(),
                genesis,
                key(),
                7,
                Hash256::repeat_byte(1),
                false
            )
            .is_err()
        );
        prepare(dir.path(), genesis, key(), 7, Hash256::ZERO, true).unwrap();
        assert!(prepare(dir.path(), genesis, key(), 8, Hash256::ZERO, true).is_err());
        assert!(
            prepare(
                dir.path(),
                genesis,
                key(),
                7,
                Hash256::repeat_byte(1),
                false
            )
            .is_err()
        );
    }
    #[test]
    fn corrupt_seed_and_exhaustion_fail_closed() {
        let seed = Zeroizing::new([1u8; 32]);
        let value = Hash256::from_slice(seed.as_ref());
        let mut chain = StoredChain {
            version: 1,
            genesis_validators_root: Hash256::ZERO,
            pubkey: key(),
            validator_index: 0,
            length: 1,
            seed,
            tip: step(value),
        };
        assert!(chain.walk(value).unwrap_err().contains("exhausted"));
        chain.tip = Hash256::repeat_byte(2);
        assert!(chain.walk(Hash256::ZERO).is_err());
    }
    #[test]
    fn concurrent_initialization_preserves_one_commitment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        let handles = (0..4)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || {
                    prepare(
                        &path,
                        Hash256::repeat_byte(3),
                        key(),
                        7,
                        Hash256::ZERO,
                        true,
                    )
                    .unwrap()
                    .tip
                })
            })
            .collect::<Vec<_>>();
        let tips = handles
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert!(tips.iter().all(|tip| *tip == tips[0]));
    }

    #[test]
    fn damaged_custody_file_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let genesis = Hash256::repeat_byte(3);
        prepare(dir.path(), genesis, key(), 7, Hash256::ZERO, true).unwrap();
        let path = dir
            .path()
            .join(format!("{genesis:?}"))
            .join(format!("{}.json", key()));
        fs::write(&path, b"truncated").unwrap();
        assert!(prepare(dir.path(), genesis, key(), 7, Hash256::ZERO, true).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"truncated");
    }

    #[cfg(unix)]
    #[test]
    fn custody_files_are_private_and_reject_loose_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let genesis = Hash256::repeat_byte(3);
        prepare(dir.path(), genesis, key(), 7, Hash256::ZERO, true).unwrap();
        let path = dir
            .path()
            .join(format!("{genesis:?}"))
            .join(format!("{}.json", key()));
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(prepare(dir.path(), genesis, key(), 7, Hash256::ZERO, true).is_err());
    }
}
