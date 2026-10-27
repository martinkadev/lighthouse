use crate::OperationPool;
use ssz::{Decode, Encode};
use ssz_derive::{Decode, Encode};
use state_processing::{
    VerifySignatures,
    per_block_processing::process_operations::process_randao_commitment_registration,
};
use store::{DBColumn, Error as StoreError, StoreItem};
use types::{BeaconState, ChainSpec, EthSpec, SignedRandaoCommitmentRegistration};

/// Stored under a separate key so the existing operation-pool SSZ schema stays compatible.
#[derive(Debug, Clone, PartialEq, Encode, Decode)]
pub struct PersistedRandaoCommitmentRegistrations {
    pub registrations: Vec<SignedRandaoCommitmentRegistration>,
}

impl StoreItem for PersistedRandaoCommitmentRegistrations {
    fn db_column() -> DBColumn {
        DBColumn::OpPool
    }
    fn as_store_bytes(&self) -> Vec<u8> {
        self.as_ssz_bytes()
    }
    fn from_store_bytes(bytes: &[u8]) -> Result<Self, StoreError> {
        Self::from_ssz_bytes(bytes).map_err(Into::into)
    }
}

impl<E: EthSpec> OperationPool<E> {
    pub fn has_randao_commitment_registration(&self, index: u64) -> bool {
        self.randao_commitment_registrations
            .read()
            .contains_key(&index)
    }

    /// Called after signature and state validation. Atomic insertion preserves first-seen gossip.
    pub fn insert_randao_commitment_registration(
        &self,
        registration: SignedRandaoCommitmentRegistration,
    ) -> bool {
        use std::collections::hash_map::Entry;
        match self
            .randao_commitment_registrations
            .write()
            .entry(registration.message.validator_index)
        {
            Entry::Vacant(entry) => {
                entry.insert(registration);
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    pub fn all_randao_commitment_registrations(&self) -> Vec<SignedRandaoCommitmentRegistration> {
        let mut registrations: Vec<_> = self
            .randao_commitment_registrations
            .read()
            .values()
            .cloned()
            .collect();
        registrations.sort_by_key(|registration| registration.message.validator_index);
        registrations
    }

    /// Revalidate against this proposal's branch; included registrations remain available for reorgs.
    pub fn get_randao_commitment_registrations(
        &self,
        state: &BeaconState<E>,
        spec: &ChainSpec,
    ) -> Vec<SignedRandaoCommitmentRegistration> {
        let mut validation_state = state.clone();
        self.all_randao_commitment_registrations()
            .into_iter()
            .filter(|registration| {
                process_randao_commitment_registration(
                    &mut validation_state,
                    registration,
                    VerifySignatures::True,
                    spec,
                )
                .is_ok()
            })
            .take(E::max_randao_commitment_registrations())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::{Hash256, MinimalEthSpec, RandaoCommitmentRegistration};

    #[test]
    fn randao_registration_pool_preserves_first_seen_and_roundtrips() {
        let pool = OperationPool::<MinimalEthSpec>::new();
        let mut registration = SignedRandaoCommitmentRegistration::empty();
        registration.message = RandaoCommitmentRegistration {
            validator_index: 3,
            commitment: Hash256::repeat_byte(42),
        };
        assert!(pool.insert_randao_commitment_registration(registration.clone()));
        let mut duplicate = registration.clone();
        duplicate.message.commitment = Hash256::repeat_byte(43);
        assert!(!pool.insert_randao_commitment_registration(duplicate));
        let persisted = PersistedRandaoCommitmentRegistrations {
            registrations: pool.all_randao_commitment_registrations(),
        };
        let restored =
            PersistedRandaoCommitmentRegistrations::from_store_bytes(&persisted.as_store_bytes())
                .unwrap();
        assert_eq!(restored.registrations, vec![registration]);
    }
}
