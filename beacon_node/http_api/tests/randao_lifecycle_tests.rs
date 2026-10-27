//! Real VC custody/signing + HTTP + beacon state transition coverage for EIP-8321.
use http_api::test_utils::InteractiveTester;
use slot_clock::{ManualSlotClock, SlotClock};
use std::sync::Arc;
use std::time::Duration;
use types::{Epoch, EthSpec, ForkName, Hash256, MinimalEthSpec, Slot};
use validator_store::{SignedBlock, UnsignedBlock, ValidatorStore};
use validator_test_rig::validator_client_harness::create_validator_store_with_genesis;

type E = MinimalEthSpec;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn randao_vc_http_registration_activation_proposal_restart_and_reorg() {
    let spec = ForkName::Heze.make_genesis_spec(E::default_spec());
    let mut tester = InteractiveTester::<E>::new(Some(spec.clone()), 64).await;
    let harness = &tester.harness;
    harness
        .mock_execution_layer
        .as_ref()
        .unwrap()
        .server
        .all_payloads_valid();
    let custody = tempfile::tempdir().unwrap();
    let config = lighthouse_validator_store::Config {
        randao_hash_chain_dir: Some(custody.path().to_path_buf()),
        ..Default::default()
    };
    let genesis_state = harness.get_current_state();
    let genesis = genesis_state.genesis_validators_root();
    let clock = ManualSlotClock::new(Slot::new(0), Duration::ZERO, spec.get_slot_duration());
    let (store, pubkeys, keys_dir) = create_validator_store_with_genesis::<E>(
        clock.clone(),
        Arc::new(spec.clone()),
        harness.runtime.task_executor.clone(),
        64,
        &config,
        genesis,
    )
    .await;

    // Register all validators through the real API, signed by the actual validator store.
    for (index, pubkey) in pubkeys.iter().enumerate() {
        let context = eth2::types::RandaoProposalContext {
            parent_root: harness.chain.head_beacon_block_root(),
            validator_index: index as u64,
            pubkey: *pubkey,
            genesis_validators_root: genesis,
            commitment: Hash256::ZERO,
            pending_commitment: None,
        };
        let prepared = store
            .prepare_randao_proposal(*pubkey, Epoch::new(0), context)
            .await
            .unwrap();
        assert!(prepared.hash_chain_reveal.is_none());
        assert!(!prepared.bls_reveal.is_infinity());
        let registration = prepared.registration.unwrap();
        tester
            .client
            .post_randao_commitment_registration(&registration)
            .await
            .unwrap();
        tester
            .client
            .post_randao_commitment_registration(&registration)
            .await
            .unwrap();
        if index == 0 {
            let mut conflicting = registration.clone();
            conflicting.message.commitment = Hash256::repeat_byte(99);
            assert!(
                tester
                    .client
                    .post_randao_commitment_registration(&conflicting)
                    .await
                    .is_err()
            );
        }
    }
    assert_eq!(
        harness
            .chain
            .op_pool
            .all_randao_commitment_registrations()
            .len(),
        64
    );
    // Both first submissions and identical retries must publish. In particular, a pooled
    // registration must not become stranded after an earlier network-publication failure.
    let mut published_registrations = 0;
    while let Ok(message) = tester.network_rx.network_recv.try_recv() {
        if let network::NetworkMessage::Publish { messages } = message {
            published_registrations += messages
                .iter()
                .filter(|message| {
                    matches!(
                        message,
                        lighthouse_network::PubsubMessage::RandaoCommitmentRegistration(_)
                    )
                })
                .count();
        }
    }
    assert_eq!(published_registrations, 128);

    let slot = Slot::new(1);
    harness.set_current_slot(slot);
    clock.set_slot(slot.as_u64());
    let context = tester
        .client
        .get_randao_proposal_context(slot)
        .await
        .unwrap();
    assert_eq!(context.commitment, Hash256::ZERO);
    let prepared = store
        .prepare_randao_proposal(context.pubkey, Epoch::new(0), context.clone())
        .await
        .unwrap();
    let (response, _) = tester
        .client
        .post_validator_blocks_v4_with_hash_chain::<E>(
            slot,
            &prepared.bls_reveal.into(),
            None,
            false,
            &eth2::types::BuilderConfig::empty(),
            None,
            ForkName::Heze,
            prepared.hash_chain_reveal,
        )
        .await
        .unwrap();
    let block = response.into_block();
    assert!(
        !block
            .body()
            .randao_commitment_registrations()
            .unwrap()
            .is_empty()
    );
    let signed = store
        .sign_block(
            context.pubkey,
            UnsignedBlock::Full(eth2::types::FullBlockContents::Block(block)),
            slot,
            None,
        )
        .await
        .unwrap();
    let SignedBlock::Full(signed) = signed else {
        panic!("full block expected")
    };
    let signed_block = signed.signed_block().clone();
    harness
        .process_block(slot, signed_block.canonical_root(), (signed_block, None))
        .await
        .unwrap();
    assert!(
        harness
            .get_current_state()
            .pending_randao_commitments()
            .unwrap()
            .len()
            > 0
    );

    // Skip to activation. Context advances the actual parent state across the epoch boundary.
    let activation = E::commitment_registration_delay().saturating_mul(E::slots_per_epoch());
    let mut selected = None;
    for number in activation..activation + 128 {
        let slot = Slot::new(number);
        harness.set_current_slot(slot);
        clock.set_slot(slot.as_u64());
        let context = tester
            .client
            .get_randao_proposal_context(slot)
            .await
            .unwrap();
        if context.commitment != Hash256::ZERO {
            selected = Some((slot, context));
            break;
        }
    }
    let (slot, context) = selected.expect("registered proposer after activation");
    let prepared = store
        .prepare_randao_proposal(
            context.pubkey,
            slot.epoch(E::slots_per_epoch()),
            context.clone(),
        )
        .await
        .unwrap();
    let reveal = prepared.hash_chain_reveal.unwrap();
    assert!(prepared.bls_reveal.is_infinity());
    // Wrong and absent reveals are rejected even when skip_randao_verification is requested.
    assert!(
        tester
            .client
            .post_validator_blocks_v4_modular_with_hash_chain::<E>(
                slot,
                &prepared.bls_reveal.clone().into(),
                None,
                eth2::types::SkipRandaoVerification::Yes,
                false,
                &eth2::types::BuilderConfig::empty(),
                None,
                ForkName::Heze,
                Some(Hash256::repeat_byte(99))
            )
            .await
            .is_err()
    );
    let (response, _) = tester
        .client
        .post_validator_blocks_v4_ssz_with_hash_chain::<E>(
            slot,
            &prepared.bls_reveal.into(),
            None,
            false,
            &eth2::types::BuilderConfig::empty(),
            None,
            ForkName::Heze,
            Some(reveal),
        )
        .await
        .unwrap();
    let block = response.into_block();
    assert_eq!(*block.body().hash_chain_reveal().unwrap(), reveal);
    let signed = store
        .sign_block(
            context.pubkey,
            UnsignedBlock::Full(eth2::types::FullBlockContents::Block(block)),
            slot,
            None,
        )
        .await
        .unwrap();
    let SignedBlock::Full(signed) = signed else {
        panic!("full block expected")
    };
    let signed_block = signed.signed_block().clone();
    harness
        .process_block(slot, signed_block.canonical_root(), (signed_block, None))
        .await
        .unwrap();
    assert_eq!(
        *harness
            .get_current_state()
            .randao_commitments()
            .unwrap()
            .get(context.validator_index as usize)
            .unwrap(),
        reveal
    );

    // Reopen both the custody files and the same slashing database after dropping the VC store.
    drop(store);
    let definitions =
        account_utils::validator_definitions::ValidatorDefinitions::open_or_create(keys_dir.path())
            .unwrap();
    let initialized = initialized_validators::InitializedValidators::from_definitions(
        definitions,
        keys_dir.path().into(),
        Default::default(),
    )
    .await
    .unwrap();
    let db = slashing_protection::SlashingDatabase::open_or_create(
        &keys_dir
            .path()
            .join(slashing_protection::SLASHING_PROTECTION_FILENAME),
    )
    .unwrap();
    let resumed = lighthouse_validator_store::LighthouseValidatorStore::<_, E>::new(
        initialized,
        db,
        genesis,
        Arc::new(spec),
        None,
        clock.clone(),
        &config,
        harness.runtime.task_executor.clone(),
    );
    for (index, pubkey) in pubkeys.iter().enumerate() {
        resumed.set_validator_index(pubkey, index as u64);
    }
    let mut next = context.clone();
    next.commitment = reveal;
    let next_prepared = resumed
        .prepare_randao_proposal(context.pubkey, slot.epoch(E::slots_per_epoch()), next)
        .await
        .unwrap();
    assert_ne!(next_prepared.hash_chain_reveal, Some(reveal));
    // A reorg restoring the original commitment must reuse that branch's preimage, not consume
    // another link. No signing occurs here: slashing protection remains in force independently.
    let restored = resumed
        .prepare_randao_proposal(context.pubkey, slot.epoch(E::slots_per_epoch()), context)
        .await
        .unwrap();
    assert_eq!(restored.hash_chain_reveal, Some(reveal));
    // Import a competing late block descending from genesis, without the original registration.
    // Proposer boost selects this branch: the registration is included again but is only pending.
    let reorg_slot = slot + 1;
    harness.set_current_slot(reorg_slot);
    let (competing, mut competing_state) = harness.make_block(genesis_state, reorg_slot).await;
    let competing_root = competing.0.canonical_root();
    let competing_hash = harness
        .process_block(reorg_slot, competing_root, competing)
        .await
        .unwrap();
    let state_root = competing_state.canonical_root().unwrap();
    let attestations = harness.make_attestations(
        &(0..64).collect::<Vec<_>>(),
        &competing_state,
        state_root,
        competing_hash,
        reorg_slot,
    );
    harness.process_attestations(attestations, &competing_state);
    harness.set_current_slot(reorg_slot + 1);
    harness.chain.recompute_head_at_current_slot().await;
    assert_eq!(harness.chain.head_beacon_block_root(), competing_root);
    let after_reorg_slot = reorg_slot + 1;
    harness.set_current_slot(after_reorg_slot);
    let after_reorg = tester
        .client
        .get_randao_proposal_context(after_reorg_slot)
        .await
        .unwrap();
    assert_eq!(after_reorg.commitment, Hash256::ZERO);
    let legacy = resumed
        .prepare_randao_proposal(
            after_reorg.pubkey,
            after_reorg_slot.epoch(E::slots_per_epoch()),
            after_reorg.clone(),
        )
        .await
        .unwrap();
    assert!(legacy.hash_chain_reveal.is_none());
    assert!(!legacy.bls_reveal.is_infinity());
    // Complete a fresh proposal through HTTP and the restarted slashing-protected VC on the
    // newly selected branch, rather than only inspecting the selected reveal.
    clock.set_slot(after_reorg_slot.as_u64());
    let (response, _) = tester
        .client
        .post_validator_blocks_v4_with_hash_chain::<E>(
            after_reorg_slot,
            &legacy.bls_reveal.into(),
            None,
            false,
            &eth2::types::BuilderConfig::empty(),
            None,
            ForkName::Heze,
            legacy.hash_chain_reveal,
        )
        .await
        .unwrap();
    let block = response.into_block();
    assert_eq!(*block.body().hash_chain_reveal().unwrap(), Hash256::ZERO);
    let signed = resumed
        .sign_block(
            after_reorg.pubkey,
            UnsignedBlock::Full(eth2::types::FullBlockContents::Block(block)),
            after_reorg_slot,
            None,
        )
        .await
        .unwrap();
    let SignedBlock::Full(signed) = signed else {
        panic!("full block expected")
    };
    let signed_block = signed.signed_block().clone();
    harness
        .process_block(
            after_reorg_slot,
            signed_block.canonical_root(),
            (signed_block, None),
        )
        .await
        .unwrap();
}
