use crate::common::{self, Protocol};
use lighthouse_network::{
    NetworkEvent,
    types::{GossipKind, PubsubMessage},
};
use std::{sync::Arc, time::Duration};
use tokio::{
    runtime::Runtime,
    time::{interval, timeout},
};
use types::{EthSpec, ForkName, Hash256, SignedRandaoCommitmentRegistration, Spec};

#[test]
fn randao_registration_propagates_between_heze_peers() {
    let _tracing =
        common::build_tracing_subscriber("lighthouse_network=debug,libp2p_gossipsub=debug", true);
    let runtime = Arc::new(Runtime::new().unwrap());
    runtime.block_on(async {
        let spec = Arc::new(common::spec_with_all_forks_enabled());
        let (mut sender, mut receiver) = common::build_node_pair(
            Arc::downgrade(&runtime),
            ForkName::Heze,
            spec.clone(),
            Protocol::Tcp,
            true,
            None,
        )
        .await;
        let slot = spec
            .heze_fork_epoch
            .unwrap()
            .start_slot(Spec::slots_per_epoch());
        let fork_id = spec.enr_fork_id::<Spec>(slot, Hash256::ZERO);
        sender.update_fork_version(fork_id.clone());
        receiver.update_fork_version(fork_id);
        sender.subscribe_kind(GossipKind::RandaoCommitmentRegistration);
        receiver.subscribe_kind(GossipKind::RandaoCommitmentRegistration);
        // Network startup may already subscribe the current-fork core topics.
        assert!(
            sender
                .gossipsub()
                .topics()
                .any(|topic| topic.as_str().contains("randao_commitment_registration"))
        );
        assert!(
            receiver
                .gossipsub()
                .topics()
                .any(|topic| topic.as_str().contains("randao_commitment_registration"))
        );

        timeout(Duration::from_secs(15), async {
            let mut tick = interval(Duration::from_millis(20));
            loop {
                let topic = sender
                    .gossipsub()
                    .topics()
                    .find(|topic| topic.as_str().contains("randao_commitment_registration"))
                    .cloned();
                if topic.is_some_and(|topic| sender.gossipsub().mesh_peers(&topic).next().is_some())
                {
                    break;
                }
                tokio::select! {
                    _ = sender.next_event() => {},
                    _ = receiver.next_event() => {},
                    _ = tick.tick() => {},
                }
            }
        })
        .await
        .expect("registration peers must join the gossip mesh");

        // This checks real transport/SSZ decoding. Consensus signature validation
        // is covered separately by the beacon-chain and EF registration tests.
        let mut registration = SignedRandaoCommitmentRegistration::empty();
        registration.message.validator_index = 7;
        registration.message.commitment = Hash256::repeat_byte(42);
        sender.publish(vec![PubsubMessage::RandaoCommitmentRegistration(Box::new(
            registration.clone(),
        ))]);
        let received = timeout(Duration::from_secs(15), async {
            loop {
                tokio::select! {
                    _ = sender.next_event() => {},
                    event = receiver.next_event() => {
                        if let NetworkEvent::PubsubMessage {
                            message: PubsubMessage::RandaoCommitmentRegistration(message), ..
                        } = event {
                            return *message;
                        }
                    }
                }
            }
        })
        .await
        .expect("registration must propagate and decode");
        assert_eq!(received, registration);
    });
}
