//! Local QA driver using the same production runtime as the GUI. Not shipped.
use anyhow::{bail, Context, Result};
use hh_clicker_launcher::{config::Layout, runtime::Runtime, setup};
use std::path::PathBuf;
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let verb = args.next().context("verb")?;
    let root = PathBuf::from(args.next().context("absolute portable root")?);
    if !root.is_absolute() {
        bail!("absolute root required");
    }
    let mut layout = Layout::new(root, None);
    if verb == "extract" {
        let payload = hh_clicker_launcher::bundle::ensure(&layout.root)?;
        println!("{}", payload.display());
        return Ok(());
    }
    if let Some(payload) = std::env::var_os("HH_RUST_QA_PAYLOAD") {
        layout.payload = PathBuf::from(payload);
    }
    let config = layout.load()?;
    let mut runtime = Runtime::new(layout.clone(), config, false);
    match verb.as_str() {
        "updates" => {
            let result = hh_clicker_launcher::updates::check()?;
            println!(
                "latest={} available={} critical={} {}",
                result.latest, result.available, result.critical, result.message
            );
        }
        "hold-port" => {
            let _listener =
                std::net::TcpListener::bind(("127.0.0.1", runtime.state.config.backend_port))?;
            println!("Holding {}", runtime.state.config.backend_port);
            std::thread::sleep(std::time::Duration::from_secs(180));
        }
        "inspect" => {
            runtime.preflight()?;
            for c in &runtime.state.checks {
                println!(
                    "{} | {} | {}",
                    if c.passed { "PASS" } else { "FAIL" },
                    c.name,
                    c.message
                );
            }
            println!("READY={}", runtime.state.ready);
        }
        "prepare-browser" => setup::prepare_browser(&layout, |p| println!("{p}"))?,
        "prepare-node" => setup::prepare_node(&layout, |p| println!("{p}"))?,
        "plan" => {
            layout.verify_payload()?;
            runtime.prepare_configuration()?;
            println!(
                "{}",
                serde_json::to_string_pretty(&runtime.isolation_summary()?)?
            );
        }
        "start" => runtime.start(|s| {
            println!(
                "{} | {:?}",
                s.message,
                s.components.iter().map(|c| c.status).collect::<Vec<_>>()
            )
        })?,
        "observe" => {
            runtime.observe()?;
            for (i, c) in runtime.state.components.iter().enumerate() {
                println!(
                    "{} | {:?} | {}",
                    hh_clicker_launcher::runtime::NAMES[i],
                    c.status,
                    c.details
                );
            }
            println!("{}", runtime.state.auth.message);
        }
        "stop" => runtime.stop(|s| println!("{}", s.message))?,
        _ => bail!("unknown action"),
    }
    Ok(())
}
