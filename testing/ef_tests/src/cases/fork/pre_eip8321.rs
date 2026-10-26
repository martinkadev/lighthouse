use milhouse::{List, ProgressiveList, Vector};
use ssz_derive::Decode;
use ssz_types::{BitVector, FixedVector};
use std::sync::Arc;
use types::{
    BalancesGloas, BeaconBlockHeader, BeaconState, BeaconStateHeze, Builder, BuilderIndex,
    BuilderPendingPayment, BuilderPendingWithdrawal, Checkpoint, Epoch, EthSpec,
    ExecutionBlockHash, ExecutionPayloadBidHeze, Fork, Hash256, HistoricalSummary,
    ParticipationFlags, PendingConsolidation, PendingDeposit, PendingPartialWithdrawal, Slot,
    SyncCommittee, ValidatorsGloas, Withdrawal,
};

/// The Heze wire layout before EIP-8321 adds its two RANDAO lists.
///
/// This view is only for loading feature-upgrade fixtures. The existing runtime Heze
/// representation already contains the feature fields; it cannot decode these pre-states.
#[derive(Decode)]
pub(super) struct PreEip8321State<E: EthSpec> {
    genesis_time: u64,
    genesis_validators_root: Hash256,
    slot: Slot,
    fork: Fork,
    latest_block_header: BeaconBlockHeader,
    block_roots: Vector<Hash256, E::SlotsPerHistoricalRoot>,
    state_roots: Vector<Hash256, E::SlotsPerHistoricalRoot>,
    historical_roots: List<Hash256, E::HistoricalRootsLimit>,
    validators: ValidatorsGloas,
    balances: BalancesGloas,
    randao_mixes: Vector<Hash256, E::EpochsPerHistoricalVector>,
    slashings: Vector<u64, E::EpochsPerSlashingsVector>,
    previous_epoch_participation: ProgressiveList<ParticipationFlags>,
    current_epoch_participation: ProgressiveList<ParticipationFlags>,
    justification_bits: BitVector<E::JustificationBitsLength>,
    previous_justified_checkpoint: Checkpoint,
    current_justified_checkpoint: Checkpoint,
    finalized_checkpoint: Checkpoint,
    inactivity_scores: ProgressiveList<u64>,
    current_sync_committee: Arc<SyncCommittee<E>>,
    next_sync_committee: Arc<SyncCommittee<E>>,
    latest_block_hash: ExecutionBlockHash,
    next_withdrawal_index: u64,
    next_withdrawal_validator_index: u64,
    historical_summaries: List<HistoricalSummary, E::HistoricalRootsLimit>,
    deposit_balance_to_consume: u64,
    exit_balance_to_consume: u64,
    earliest_exit_epoch: Epoch,
    consolidation_balance_to_consume: u64,
    earliest_consolidation_epoch: Epoch,
    pending_deposits: ProgressiveList<PendingDeposit>,
    pending_partial_withdrawals: ProgressiveList<PendingPartialWithdrawal>,
    pending_consolidations: ProgressiveList<PendingConsolidation>,
    proposer_lookahead: Vector<u64, E::ProposerLookaheadSlots>,
    builders: ProgressiveList<Builder>,
    next_withdrawal_builder_index: BuilderIndex,
    execution_payload_availability: BitVector<E::SlotsPerHistoricalRoot>,
    builder_pending_payments: Vector<BuilderPendingPayment, E::BuilderPendingPaymentsLimit>,
    builder_pending_withdrawals: ProgressiveList<BuilderPendingWithdrawal>,
    latest_execution_payload_bid: ExecutionPayloadBidHeze<E>,
    payload_expected_withdrawals: ProgressiveList<Withdrawal>,
    ptc_window: Vector<FixedVector<u64, E::PTCSize>, E::PtcWindowLength>,
}

impl<E: EthSpec> PreEip8321State<E> {
    /// Transfer the decoded Heze fields into the runtime feature representation.
    ///
    /// The two empty lists are placeholders for feature activation, not pre-state SSZ fields.
    pub(super) fn into_feature_state(self) -> BeaconState<E> {
        BeaconState::Heze(BeaconStateHeze {
            genesis_time: self.genesis_time,
            genesis_validators_root: self.genesis_validators_root,
            slot: self.slot,
            fork: self.fork,
            latest_block_header: self.latest_block_header,
            block_roots: self.block_roots,
            state_roots: self.state_roots,
            historical_roots: self.historical_roots,
            validators: self.validators,
            balances: self.balances,
            randao_mixes: self.randao_mixes,
            slashings: self.slashings,
            previous_epoch_participation: self.previous_epoch_participation,
            current_epoch_participation: self.current_epoch_participation,
            justification_bits: self.justification_bits,
            previous_justified_checkpoint: self.previous_justified_checkpoint,
            current_justified_checkpoint: self.current_justified_checkpoint,
            finalized_checkpoint: self.finalized_checkpoint,
            inactivity_scores: self.inactivity_scores,
            current_sync_committee: self.current_sync_committee,
            next_sync_committee: self.next_sync_committee,
            latest_block_hash: self.latest_block_hash,
            next_withdrawal_index: self.next_withdrawal_index,
            next_withdrawal_validator_index: self.next_withdrawal_validator_index,
            historical_summaries: self.historical_summaries,
            deposit_balance_to_consume: self.deposit_balance_to_consume,
            exit_balance_to_consume: self.exit_balance_to_consume,
            earliest_exit_epoch: self.earliest_exit_epoch,
            consolidation_balance_to_consume: self.consolidation_balance_to_consume,
            earliest_consolidation_epoch: self.earliest_consolidation_epoch,
            pending_deposits: self.pending_deposits,
            pending_partial_withdrawals: self.pending_partial_withdrawals,
            pending_consolidations: self.pending_consolidations,
            proposer_lookahead: self.proposer_lookahead,
            builders: self.builders,
            next_withdrawal_builder_index: self.next_withdrawal_builder_index,
            execution_payload_availability: self.execution_payload_availability,
            builder_pending_payments: self.builder_pending_payments,
            builder_pending_withdrawals: self.builder_pending_withdrawals,
            latest_execution_payload_bid: self.latest_execution_payload_bid,
            payload_expected_withdrawals: self.payload_expected_withdrawals,
            ptc_window: self.ptc_window,
            randao_commitments: ProgressiveList::empty(),
            pending_randao_commitments: ProgressiveList::empty(),
            total_active_balance: None,
            committee_caches: std::array::from_fn(|_| Arc::new(<_>::default())),
            progressive_balances_cache: <_>::default(),
            pubkey_cache: <_>::default(),
            exit_cache: <_>::default(),
            slashings_cache: <_>::default(),
            epoch_cache: <_>::default(),
        })
    }
}
