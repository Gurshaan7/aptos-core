// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::{three_region_simulation_test::ExecutionDelayConfig, LoadDestination, NetworkLoadTest};
use aptos_forge::{
    GroupNetworkDelay, NetworkContext, NetworkTest, Swarm, SwarmChaos, SwarmExt,
    SwarmNetworkBandwidth, SwarmNetworkDelay, Test,
};
use aptos_logger::info;
use aptos_types::PeerId;
use csv::Reader;
use itertools::{self, Itertools};
use rand::Rng;
use std::collections::BTreeMap;
use tokio::runtime::Runtime;

macro_rules! LATENCY_TABLE_CSV {
    () => {
        "data/latency_table.csv"
    };
}

pub struct MultiRegionSimulationTest {
    pub add_execution_delay: Option<ExecutionDelayConfig>,
}

impl Test for MultiRegionSimulationTest {
    fn name(&self) -> &'static str {
        "network::multi-region-simulation"
    }
}

fn get_latency_table() -> BTreeMap<String, BTreeMap<String, u64>> {
    let mut latency_table = BTreeMap::new();

    let mut rdr = Reader::from_reader(include_bytes!(LATENCY_TABLE_CSV!()).as_slice());
    for result in rdr.deserialize() {
        if let Ok((from, to, latency)) = result {
            let from: String = from;
            let to: String = to;
            let latency: f64 = latency;
            latency_table
                .entry(from)
                .or_insert_with(BTreeMap::new)
                .insert(to, latency as u64);
        }
    }

    latency_table
}

/// Creates a SwarmNetworkDelay
fn create_multi_region_swarm_network_delay(swarm: &dyn Swarm) -> SwarmNetworkDelay {
    let latency_table = get_latency_table();

    let all_validators = swarm.validators().map(|v| v.peer_id()).collect::<Vec<_>>();
    assert!(all_validators.len() > latency_table.len());

    let number_of_regions = latency_table.len();
    let approx_validators_per_region = all_validators.len() / number_of_regions;

    let validator_chunks = all_validators.chunks_exact(approx_validators_per_region);

    let mut group_network_delays: Vec<GroupNetworkDelay> = validator_chunks
        .clone()
        .zip(latency_table)
        .combinations(2)
        .map(|perm| {
            let (from_chunk, (from_region, to_latencies)) = &perm[0];
            let (to_chunk, (to_region, _)) = &perm[1];

            let latency = to_latencies[to_region];
            let delay = [
                GroupNetworkDelay {
                    name: format!("{}-to-{}", from_region.clone(), to_region.clone()),
                    source_nodes: from_chunk.to_vec(),
                    target_nodes: to_chunk.to_vec(),
                    latency_ms: latency,
                    jitter_ms: 5,
                    correlation_percentage: 50,
                },
                GroupNetworkDelay {
                    name: format!("{}-to-{}", to_region.clone(), from_region.clone()),
                    source_nodes: to_chunk.to_vec(),
                    target_nodes: from_chunk.to_vec(),
                    latency_ms: latency,
                    jitter_ms: 5,
                    correlation_percentage: 50,
                },
            ];
            info!("{:?}", delay);

            delay
        })
        .flatten()
        .collect();

    let remainder = validator_chunks.remainder();
    let remaining_validators: Vec<PeerId> = validator_chunks
        .skip(number_of_regions)
        .flatten()
        .chain(remainder.into_iter())
        .cloned()
        .collect();
    info!("remaining: {:?}", remaining_validators);
    if remaining_validators.len() > 0 {
        group_network_delays[0]
            .source_nodes
            .append(remaining_validators.to_vec().as_mut());
        group_network_delays[1]
            .target_nodes
            .append(remaining_validators.to_vec().as_mut());
    }

    assert_eq!(
        group_network_delays.len(),
        number_of_regions * number_of_regions - number_of_regions
    );

    info!(
        "network_delays {}: {:?}",
        group_network_delays.len(),
        group_network_delays
    );

    SwarmNetworkDelay {
        group_network_delays,
    }
}

// 1 Gbps
fn create_bandwidth_limit() -> SwarmNetworkBandwidth {
    SwarmNetworkBandwidth {
        rate: 1000,
        limit: 20971520,
        buffer: 10000,
    }
}

fn add_execution_delay(swarm: &mut dyn Swarm, config: &ExecutionDelayConfig) -> anyhow::Result<()> {
    let runtime = Runtime::new().unwrap();
    let validators = swarm.get_validator_clients_with_names();

    runtime.block_on(async {
        let mut rng = rand::thread_rng();
        for (name, validator) in validators {
            let sleep_fraction = if rng.gen_bool(config.inject_delay_node_fraction) {
                rng.gen_range(1_u32, config.inject_delay_max_transaction_percentage)
            } else {
                0
            };
            let name = name.clone();
            info!(
                "Validator {} adding {}% of transactions with 1ms execution delay",
                name, sleep_fraction
            );
            validator
                .set_failpoint(
                    "aptos_vm::execution::user_transaction".to_string(),
                    format!(
                        "{}%delay({})",
                        sleep_fraction, config.inject_delay_per_transaction_ms
                    ),
                )
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "set_failpoint to add execution delay on {} failed, {:?}",
                        name,
                        e
                    )
                })?;
        }
        Ok::<(), anyhow::Error>(())
    })
}

fn remove_execution_delay(swarm: &mut dyn Swarm) -> anyhow::Result<()> {
    let runtime = Runtime::new().unwrap();
    let validators = swarm.get_validator_clients_with_names();

    runtime.block_on(async {
        for (name, validator) in validators {
            let name = name.clone();

            validator
                .set_failpoint(
                    "aptos_vm::execution::block_metadata".to_string(),
                    "off".to_string(),
                )
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "set_failpoint to remove execution delay on {} failed, {:?}",
                        name,
                        e
                    )
                })?;
        }
        Ok::<(), anyhow::Error>(())
    })
}

impl NetworkLoadTest for MultiRegionSimulationTest {
    fn setup(&self, ctx: &mut NetworkContext) -> anyhow::Result<LoadDestination> {
        // inject network delay
        let delay = create_multi_region_swarm_network_delay(ctx.swarm());
        let chaos = SwarmChaos::Delay(delay);
        ctx.swarm().inject_chaos(chaos)?;

        // inject bandwidth limit
        let bandwidth = create_bandwidth_limit();
        let chaos = SwarmChaos::Bandwidth(bandwidth);
        ctx.swarm().inject_chaos(chaos)?;

        if let Some(config) = &self.add_execution_delay {
            add_execution_delay(ctx.swarm(), config)?;
        }

        Ok(LoadDestination::FullnodesOtherwiseValidators)
    }

    fn finish(&self, swarm: &mut dyn Swarm) -> anyhow::Result<()> {
        if self.add_execution_delay.is_some() {
            remove_execution_delay(swarm)?;
        }

        swarm.remove_all_chaos()
    }
}

impl NetworkTest for MultiRegionSimulationTest {
    fn run<'t>(&self, ctx: &mut NetworkContext<'t>) -> anyhow::Result<()> {
        <dyn NetworkLoadTest>::run(self, ctx)
    }
}

// #[test]
// fn test_my_func() {
//     let mut logger = aptos_logger::Logger::new();
//     logger
//         .channel_size(1000)
//         .is_async(false)
//         .level(aptos_logger::Level::Info);
//     logger.build();
//     create_multi_region_swarm_network_delay2();
// }