use serde_json::Value;
use std::process::Command;

fn sim_bin() -> std::path::PathBuf {
    std::env::var_os("CARGO_BIN_EXE_market-testnet-sim")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let exe = std::env::current_exe().expect("test executable path");
            let deps = exe.parent().expect("deps directory");
            let debug = deps.parent().expect("debug directory");
            debug.join(format!(
                "market-testnet-sim{}",
                std::env::consts::EXE_SUFFIX
            ))
        })
}

#[test]
fn quick_market_testnet_simulation_writes_auditable_metrics() {
    let output = Command::new(sim_bin())
        .arg("--quick")
        .env("KAEL_SIM_SEED", "7")
        .output()
        .expect("run market testnet simulation");

    assert!(
        output.status.success(),
        "simulation failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let metrics_path = "/tmp/kael-30node-market-testnet-simulation/metrics.json";
    let metrics: Value = serde_json::from_slice(
        &std::fs::read(metrics_path).expect("simulation metrics must exist"),
    )
    .expect("metrics must be valid JSON");

    assert_eq!(metrics["logical_nodes"], 5);
    assert_eq!(metrics["wallets"], 5);
    assert_eq!(metrics["preflight_zero_tx"], "PASS");
    assert_eq!(metrics["reorg_simulation_status"], "PASS");
    assert_eq!(metrics["orderbook_price_time_status"], "PASS");
    assert_eq!(metrics["settlement_status"], "PASS");
    assert_eq!(metrics["erc20_status"], "PASS");
    assert_eq!(metrics["secret_leaks_detected"], 0);
    assert_eq!(metrics["unsafe_broadcasts_detected"], 0);
    assert_eq!(metrics["unexpected_failures"], 0);
    assert_eq!(metrics["verdict"], "PASS");
}
