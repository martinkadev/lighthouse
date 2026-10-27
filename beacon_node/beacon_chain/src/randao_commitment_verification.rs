use crate::{BeaconChain, BeaconChainTypes};
use state_processing::{
    VerifySignatures,
    per_block_processing::{
        errors::{BlockProcessingError, RandaoCommitmentRegistrationInvalidReason},
        process_operations::process_randao_commitment_registration,
    },
};
use types::SignedRandaoCommitmentRegistration;

#[derive(Debug)]
pub enum RandaoCommitmentGossipError {
    AlreadySeen,
    PreFork,
    Invalid(BlockProcessingError),
}

impl RandaoCommitmentGossipError {
    pub fn should_reject(&self) -> bool {
        matches!(
            self,
            Self::Invalid(BlockProcessingError::RandaoCommitmentRegistrationInvalid {
                reason: RandaoCommitmentRegistrationInvalidReason::ValidatorUnknown
                    | RandaoCommitmentRegistrationInvalidReason::CommitmentUnset
                    | RandaoCommitmentRegistrationInvalidReason::BadSignature,
                ..
            })
        )
    }
}

impl<T: BeaconChainTypes> BeaconChain<T> {
    pub fn verify_and_import_randao_commitment_registration(
        &self,
        registration: SignedRandaoCommitmentRegistration,
    ) -> Result<(), RandaoCommitmentGossipError> {
        if self
            .op_pool
            .has_randao_commitment_registration(registration.message.validator_index)
        {
            return Err(RandaoCommitmentGossipError::AlreadySeen);
        }
        let head = self.head_snapshot();
        if head.beacon_state.fork().current_version < self.spec.heze_fork_version {
            return Err(RandaoCommitmentGossipError::PreFork);
        }
        let mut state = head.beacon_state.clone();
        process_randao_commitment_registration(
            &mut state,
            &registration,
            VerifySignatures::True,
            &self.spec,
        )
        .map_err(RandaoCommitmentGossipError::Invalid)?;
        if !self
            .op_pool
            .insert_randao_commitment_registration(registration)
        {
            return Err(RandaoCommitmentGossipError::AlreadySeen);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::BeaconChainHarness;
    use std::sync::Arc;
    use tree_hash::TreeHash;
    use types::test_utils::generate_deterministic_keypairs;
    use types::{Domain, EthSpec, ForkName, Hash256, MinimalEthSpec, SigningData};

    #[tokio::test]
    async fn registered_randao_proposer_requires_a_supported_reveal_path() {
        type E = MinimalEthSpec;
        let spec = Arc::new(ForkName::Heze.make_genesis_spec(E::default_spec()));
        let harness = BeaconChainHarness::builder(E::default())
            .spec(spec)
            .deterministic_keypairs(64)
            .fresh_ephemeral_store()
            .mock_execution_layer()
            .mock_execution_layer_all_payloads_valid()
            .build();
        let mut state = harness.get_current_state();
        for index in 0..state.validators().len() {
            *state
                .randao_commitments_mut()
                .unwrap()
                .get_mut(index)
                .unwrap() = Hash256::repeat_byte(42);
        }
        let result = harness
            .chain
            .produce_block_on_state_gloas(
                state,
                None,
                harness.chain.head_beacon_block_root(),
                proto_array::PayloadStatus::Empty,
                None,
                types::Slot::new(1),
                bls::Signature::empty(),
                crate::graffiti_calculator::GraffitiSettings::new(None, None),
                crate::ProduceBlockVerification::VerifyRandao,
                eth2::types::BuilderConfig::empty(),
            )
            .await;
        assert!(matches!(
            result,
            Err(crate::BlockProductionError::HashChainRandaoUnsupported)
        ));
    }

    #[tokio::test]
    async fn randao_registration_gossip_pool_restart_and_branch_revalidation() {
        type E = MinimalEthSpec;
        let spec = Arc::new(ForkName::Heze.make_genesis_spec(E::default_spec()));
        let keys = generate_deterministic_keypairs(64);
        let harness = BeaconChainHarness::builder(E::default())
            .spec(spec.clone())
            .keypairs(keys.clone())
            .fresh_ephemeral_store()
            .mock_execution_layer()
            .mock_execution_layer_all_payloads_valid()
            .build();
        let state = harness.get_current_state();
        let mut registration = SignedRandaoCommitmentRegistration::empty();
        registration.message.validator_index = 0;
        registration.message.commitment = Hash256::repeat_byte(42);
        let domain = spec.compute_domain(
            Domain::RandaoCommitmentRegistration,
            spec.genesis_fork_version,
            state.genesis_validators_root(),
        );
        let signing_root = SigningData {
            object_root: registration.message.tree_hash_root(),
            domain,
        }
        .tree_hash_root();
        registration.signature = keys[0].sk.sign(signing_root);

        let mut invalid = registration.clone();
        invalid.signature = bls::Signature::empty();
        let error = harness
            .chain
            .verify_and_import_randao_commitment_registration(invalid)
            .unwrap_err();
        assert!(error.should_reject());
        assert!(!harness.chain.op_pool.has_randao_commitment_registration(0));
        harness
            .chain
            .verify_and_import_randao_commitment_registration(registration.clone())
            .unwrap();
        assert!(matches!(
            harness
                .chain
                .verify_and_import_randao_commitment_registration(registration.clone()),
            Err(RandaoCommitmentGossipError::AlreadySeen)
        ));
        assert_eq!(
            harness
                .chain
                .op_pool
                .get_randao_commitment_registrations(&state, &spec),
            vec![registration.clone()]
        );

        // Exercise the actual proposal path, not only the pool's candidate getter.
        let (contents, post_state) = harness.make_block(state.clone(), types::Slot::new(1)).await;
        assert_eq!(
            contents
                .0
                .message()
                .body()
                .randao_commitment_registrations()
                .unwrap()
                .len(),
            1
        );
        assert!(
            post_state
                .pending_randao_commitments()
                .unwrap()
                .iter()
                .any(
                    |pending| pending.validator_index == registration.message.validator_index
                        && pending.commitment == registration.message.commitment
                )
        );

        let mut included_branch = state.clone();
        process_randao_commitment_registration(
            &mut included_branch,
            &registration,
            VerifySignatures::True,
            &spec,
        )
        .unwrap();
        assert!(
            harness
                .chain
                .op_pool
                .get_randao_commitment_registrations(&included_branch, &spec)
                .is_empty()
        );
        assert_eq!(
            harness
                .chain
                .op_pool
                .get_randao_commitment_registrations(&state, &spec),
            vec![registration.clone()],
            "the registration must remain selectable after a reorg to the branch without it"
        );

        harness.chain.persist_op_pool().unwrap();
        harness.chain.persist_fork_choice().unwrap();
        let store = harness.chain.store.clone();
        let clock = harness.chain.slot_clock.clone();
        drop(harness);
        let resumed = BeaconChainHarness::builder(E::default())
            .spec(spec.clone())
            .keypairs(keys)
            .resumed_ephemeral_store(store)
            .mock_execution_layer()
            .mock_execution_layer_all_payloads_valid()
            .testing_slot_clock(clock)
            .build();
        assert_eq!(
            resumed.chain.op_pool.all_randao_commitment_registrations(),
            vec![registration.clone()]
        );
        assert!(matches!(
            resumed
                .chain
                .verify_and_import_randao_commitment_registration(registration),
            Err(RandaoCommitmentGossipError::AlreadySeen)
        ));
    }
}
