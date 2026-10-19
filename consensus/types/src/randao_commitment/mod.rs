mod pending_randao_commitment;
mod randao_commitment_registration;
mod signed_randao_commitment_registration;

pub use pending_randao_commitment::PendingRandaoCommitment;
pub use randao_commitment_registration::RandaoCommitmentRegistration;
pub use signed_randao_commitment_registration::SignedRandaoCommitmentRegistration;

pub const HASH_CHAIN_RANDAO_DST: &[u8] = b"HASH_CHAIN_RANDAO";
pub const COMMITMENT_REGISTRATION_DELAY: u64 = 3;
