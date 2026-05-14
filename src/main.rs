use anyhow::{bail, Result};
use parity_scale_codec::Decode;
use serde_json::json;
use sp_core::crypto::{AccountId32, Ss58Codec};
use sp_core::hashing::twox_128;
use std::env;
use subxt::{OnlineClient, PolkadotConfig};

const DOT_DECIMALS: u128 = 10_000_000_000;
const KSM_DECIMALS: u128 = 1_000_000_000_000;


#[derive(Debug, Decode)]
struct Candidate {
    who: AccountId32,
    deposit: u128,
}

struct ChainConfig {
    name: &'static str,
    rpc: &'static str,
    decimals: u128,
    symbol: &'static str,
}

struct NetworkConfig {
    name: &'static str,
    chains: &'static [ChainConfig],
}

struct ChainReport {
    is_active: bool,
    message: String,
}

const POLKADOT_CHAINS: &[ChainConfig] = &[
    ChainConfig {
        name: "Assethub",
        rpc: "wss://rpc-asset-hub-polkadot.luckyfriday.io",
        decimals: DOT_DECIMALS,
        symbol: "DOT",
    },
    ChainConfig {
        name: "Bridgehub",
        rpc: "wss://rpc-bridge-hub-polkadot.luckyfriday.io",
        decimals: DOT_DECIMALS,
        symbol: "DOT",
    },
    ChainConfig {
        name: "Collectives",
        rpc: "wss://rpc-collectives-polkadot.luckyfriday.io",
        decimals: DOT_DECIMALS,
        symbol: "DOT",
    },
    ChainConfig {
        name: "Coretime",
        rpc: "wss://rpc-coretime-polkadot.luckyfriday.io",
        decimals: DOT_DECIMALS,
        symbol: "DOT",
    },
    ChainConfig {
        name: "People",
        rpc: "wss://rpc-people-polkadot.luckyfriday.io",
        decimals: DOT_DECIMALS,
        symbol: "DOT",
    },
];

const KUSAMA_CHAINS: &[ChainConfig] = &[
    ChainConfig {
        name: "Assethub",
        rpc: "wss://rpc-asset-hub-kusama.luckyfriday.io",
        decimals: KSM_DECIMALS,
        symbol: "KSM",
    },
    ChainConfig {
        name: "Bridgehub",
        rpc: "wss://rpc-bridge-hub-kusama.luckyfriday.io",
        decimals: KSM_DECIMALS,
        symbol: "KSM",
    },
    ChainConfig {
        name: "Coretime",
        rpc: "wss://rpc-coretime-kusama.luckyfriday.io",
        decimals: KSM_DECIMALS,
        symbol: "KSM",
    },
    ChainConfig {
        name: "People",
        rpc: "wss://rpc-people-kusama.luckyfriday.io",
        decimals: KSM_DECIMALS,
        symbol: "KSM",
    },
    ChainConfig {
        name: "Encointer",
        rpc: "wss://rpc-encointer-kusama.luckyfriday.io",
        decimals: KSM_DECIMALS,
        symbol: "KSM",
    },
];


const NETWORKS: &[NetworkConfig] = &[
    NetworkConfig {
        name: "Polkadot",
        chains: POLKADOT_CHAINS,
    },
    NetworkConfig {
        name: "Kusama",
        chains: KUSAMA_CHAINS,
    },
];

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::from_path("/etc/opt/app/slack/.env")?;
    dotenvy::from_path("/etc/opt/app/collator/.env")?;

    let slack_webhook = env::var("SLACK_WEBHOOK")?;

    let polkadot_collator =
        AccountId32::from_ss58check(&env::var("POLKADOT_COLLATOR")?)?;

    let kusama_collator =
        AccountId32::from_ss58check(&env::var("KUSAMA_COLLATOR")?)?;

    let mut slack_message = String::new();

    for network in NETWORKS {
        let monitored = match network.name {
            "Polkadot" => &polkadot_collator,
            "Kusama" => &kusama_collator,
            _ => unreachable!(),
        };

        let mut active_count = 0;
        let mut active_chains: Vec<&str> = Vec::new();
        let mut reports: Vec<String> = Vec::new();

        for chain in network.chains {
            match process_chain(chain, monitored).await {
                Ok(report) => {
                    if report.is_active {
                        active_count += 1;
                        active_chains.push(chain.name);
                    }

                    reports.push(report.message);
                }
                Err(err) => {
                    reports.push(format!(
                        "Error processing {}: {}",
                        chain.name,
                        err
                    ));
                }
            }
        }

        slack_message.push_str(&format!(
            "*{}*\n",
            network.name
        ));

        slack_message.push_str(&format!(
            "• Collating on {}/{} chains - {}\n",
            active_count,
            network.chains.len(),
            active_chains.join(", ")
        ));

        for report in reports {
            slack_message.push_str(&format!("• {}\n", report));
        }

        slack_message.push('\n');
    }

    println!("{}", slack_message);

    send_to_slack(&slack_webhook, &slack_message).await?;

    Ok(())
}

async fn process_chain(
    chain: &ChainConfig,
    monitored: &AccountId32,
) -> Result<ChainReport> {
    let api = OnlineClient::<PolkadotConfig>::from_url(chain.rpc).await?;
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

    let in_invulnerables = invulnerables.iter().any(|a| a == monitored);
    let candidate = candidates.iter().find(|c| &c.who == monitored);
    let is_in_list = in_invulnerables || candidate.is_some();

    let max = candidates.iter().max_by_key(|c| c.deposit).unwrap();
    let min = candidates.iter().min_by_key(|c| c.deposit).unwrap();

    let message = if is_in_list {
        format!(
            "Max bid on {} is {} {} and min bid is {} {}",
            chain.name,
            fmt_token(max.deposit, chain.decimals),
            chain.symbol,
            fmt_token(min.deposit, chain.decimals),
            chain.symbol,
        )
    } else {
        format!(
            "We have dropped out of {}, max bid is {} {} and min bid is {} {}",
            chain.name,
            fmt_token(max.deposit, chain.decimals),
            chain.symbol,
            fmt_token(min.deposit, chain.decimals),
            chain.symbol,
        )
    };

    Ok(ChainReport {
        is_active: is_in_list,
        message,
    })
}

fn storage_key(pallet: &str, item: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(32);
    key.extend_from_slice(&twox_128(pallet.as_bytes()));
    key.extend_from_slice(&twox_128(item.as_bytes()));
    key
}

fn fmt_token(amount: u128, decimals: u128) -> String {
    format!(
        "{}.{:02}",
        amount / decimals,
        ((amount % decimals) * 100) / decimals
    )
}

async fn send_to_slack(webhook: &str, message: &str) -> Result<()> {
    let client = reqwest::Client::new();

    let response = client
        .post(webhook)
        .json(&json!({
            "text": message
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        bail!("slack webhook failed: {}", response.status());
    }

    Ok(())
}