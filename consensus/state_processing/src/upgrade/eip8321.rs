use milhouse::ProgressiveList;
use types::{BeaconState, BeaconStateError, EthSpec, Fork};

/// Activate the experimental EIP-8321 feature on the existing Heze representation.
///
/// This does not introduce a named consensus fork. The caller supplies the feature's
/// network-specific version and selects when activation occurs.
pub fn upgrade_to_eip8321<E: EthSpec>(
    state: &mut BeaconState<E>,
    feature_version: [u8; 4],
) -> Result<(), BeaconStateError> {
    let epoch = state.current_epoch();
    let pre = state.as_heze_mut()?;
    if pre.fork.current_version == feature_version {
        return Err(BeaconStateError::IncorrectStateVariant);
    }

    let randao_commitments = ProgressiveList::try_from_iter(std::iter::repeat_n(
        types::consts::heze::UNSET_RANDAO_COMMITMENT,
        pre.validators.len(),
    ))?;

    pre.randao_commitments = randao_commitments;
    pre.pending_randao_commitments = ProgressiveList::empty();
    pre.fork = Fork {
        previous_version: pre.fork.current_version,
        current_version: feature_version,
        epoch,
    };
    Ok(())
}
