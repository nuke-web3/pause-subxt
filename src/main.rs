// Copyright 2019-2022 Parity Technologies (UK) Ltd.
// This file is dual-licensed as Apache-2.0 or GPL-3.0.
// see LICENSE for license details.

use futures::StreamExt;
use sp_keyring::AccountKeyring;
use subxt::{
    tx::PairSigner,
    OnlineClient,
    SubstrateConfig
};


#[subxt::subxt(runtime_metadata_path = "artifacts/node-metadata.scale")]
pub mod kitchen_sink_node {}

type RuntimeCall = kitchen_sink_node::runtime_types::kitchensink_runtime::RuntimeCall;
type BalancesCall = kitchen_sink_node::runtime_types::pallet_balances::pallet::Call;
type TxPauseCall = kitchen_sink_node::runtime_types::pallet_tx_pause::pallet::Call;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // Transfers work fine
    handle_transfer_events().await?;

    // Pause transfers
    pause_simple_transfer().await?;

    // Should error with `CallFiltered`
    handle_transfer_events().await?;

    Ok(())
}

/// This is the highest level approach to using this API. We use `wait_for_finalized_success`
/// to wait for the transaction to make it into a finalized block, and also ensure that the
/// transaction was successful according to the associated events.
async fn pause_simple_transfer() -> Result<(), Box<dyn std::error::Error>> {
    // NOTE: assumes Alice is Root!
    let signer = PairSigner::new(AccountKeyring::Alice.pair());
    let api = OnlineClient::<SubstrateConfig>::new().await?;

    let bob = AccountKeyring::Bob.to_account_id().into();

    // For the sake of tx-pause we need a valid `RuntimeCall` to pass type checks,
    // but the _values_ passed to the transfer here are arbitrary when using it to
    // pass in for the tx-pause `pause_call(call: RuntimeCall)` call.
    let balance_transfer_call = RuntimeCall::Balances(BalancesCall::transfer {
        dest: bob,
        value: 10_000,
    });

    let pause_balance_transfer_call = RuntimeCall::TxPause(TxPauseCall::pause_call {
        call: Box::new(balance_transfer_call),
    });

    // The below_could_ work, if issued from the correct `PauseOrigin` configured in the node's runtime.
    // let pause_balance_transfers_tx = kitchen_sink_node::tx().tx_pause().pause_call(balance_transfer_tx.into());

    let sudo_pause_balance_transfers_tx = kitchen_sink_node::tx().sudo().sudo(pause_balance_transfer_call);

    let pause_balance_transfers = api
        .tx()
        .sign_and_submit_then_watch_default(&sudo_pause_balance_transfers_tx, &signer)
        .await?
        .wait_for_finalized_success()
        .await?;

    let transfer_event =
        pause_balance_transfers.find_first::<kitchen_sink_node::balances::events::Transfer>()?;

    if let Some(event) = transfer_event {
        println!("Balance transfer success: {event:?}");
    } else {
        println!("Failed to find Balances::Transfer Event");
    }
    Ok(())
}

/// If we need more visibility into the state of the transaction, we can also ditch
/// `wait_for_finalized` entirely and stream the transaction progress events, handling
/// them more manually.
async fn handle_transfer_events() -> Result<(), Box<dyn std::error::Error>> {
    let signer = PairSigner::new(AccountKeyring::Alice.pair());
    let bob = AccountKeyring::Bob.to_account_id().into();

    let api = OnlineClient::<SubstrateConfig>::new().await?;

    let balance_transfer_tx = kitchen_sink_node::tx().balances().transfer(bob, 10_000);

    let mut balance_transfer_progress = api
        .tx()
        .sign_and_submit_then_watch_default(&balance_transfer_tx, &signer)
        .await?;

    while let Some(ev) = balance_transfer_progress.next().await {
        let ev = ev?;
        use subxt::tx::TxStatus::*;

        // Made it into a block, but not finalized.
        if let InBlock(details) = ev {
            println!(
                "Transaction {:?} made it into block {:?}",
                details.extrinsic_hash(),
                details.block_hash()
            );

            let events = details.wait_for_success().await?;
            let transfer_event =
                events.find_first::<kitchen_sink_node::balances::events::Transfer>()?;

            if let Some(event) = transfer_event {
                println!(
                    "Balance transfer is now in block (but not finalized): {event:?}"
                );
            } else {
                println!("Failed to find Balances::Transfer Event");
            }
        }
        // Finalized!
        else if let Finalized(details) = ev {
            println!(
                "Transaction {:?} is finalized in block {:?}",
                details.extrinsic_hash(),
                details.block_hash()
            );

            let events = details.wait_for_success().await?;
            let transfer_event =
                events.find_first::<kitchen_sink_node::balances::events::Transfer>()?;

            if let Some(event) = transfer_event {
                println!("Balance transfer success: {event:?}");
            } else {
                println!("Failed to find Balances::Transfer Event");
            }
        }
        // Report other statuses we see.
        else {
            println!("Current transaction status: {:?}", ev);
        }
    }

    Ok(())
}