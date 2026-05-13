use anyhow::{bail, Result};
use clap::Parser;
use parity_scale_codec::Decode;
use sp_core::crypto::{AccountId32, Ss58Codec};
use sp_core::hashing::twox_128;
use subxt::{OnlineClient, PolkadotConfig};

const DOT: u128 = 10_000_000_000;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "wss://rpc-asset-hub-polkadot.luckyfriday.io")]
    rpc: String,

    #[arg(long)]
    collator: String,

    #[arg(long, default_value = "Assethub")]
    chain_label: String,
}

#[derive(Debug, Decode)]
struct Candidate {
    who: AccountId32,
    deposit: u128,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let monitored = AccountId32::from_ss58check(&args.collator)?;

    let api = OnlineClient::<PolkadotConfig>::from_url(args.rpc).await?;
    let at = api.at_current_block().await?;
    let storage = at.storage();

    let inv_bytes = storage
        .fetch_raw(storage_key("CollatorSelection", "Invulnerables"))
        .await?;

    let cand_bytes = storage
        .fetch_raw(storage_key("CollatorSelection", "CandidateList"))
        .await?;

    let invulnerables = Vec::<AccountId32>::decode(&mut &inv_bytes[..])?;
    let candidates = Vec::<Candidate>::decode(&mut &cand_bytes[..])?;

    if candidates.is_empty() {
        bail!("candidate list is empty");
    }

    let in_invulnerables = invulnerables.iter().any(|a| a == &monitored);
    let in_candidates = candidates.iter().any(|c| c.who == monitored);
    let is_in_list = in_invulnerables || in_candidates;

    let max = candidates.iter().max_by_key(|c| c.deposit).unwrap();
    let min = candidates.iter().min_by_key(|c| c.deposit).unwrap();

    let status = if in_invulnerables {
        "invulnerable".to_string()
    } else if let Some(candidate) = candidates.iter().find(|c| c.who == monitored) {
        format!("candidate with bid {} DOT", fmt_dot(candidate.deposit))
    } else {
        "not in the collator list".to_string()
    };

    println!("Monitored collator is {status}");

    if is_in_list {
        println!(
            "Max bid on {} is {} DOT and min bid is {} DOT",
            args.chain_label,
            fmt_dot(max.deposit),
            fmt_dot(min.deposit),
        );
    } else {
        println!(
            "We have dropped out of {}, max bid is {} DOT and min bid is {} DOT",
            args.chain_label,
            fmt_dot(max.deposit),
            fmt_dot(min.deposit),
        );
    }

    Ok(())
}

fn storage_key(pallet: &str, item: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(32);
    key.extend_from_slice(&twox_128(pallet.as_bytes()));
    key.extend_from_slice(&twox_128(item.as_bytes()));
    key
}

fn fmt_dot(plancks: u128) -> String {
    format!("{}.{:02}", plancks / DOT, ((plancks % DOT) * 100) / DOT)
}