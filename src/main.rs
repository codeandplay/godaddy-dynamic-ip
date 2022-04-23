mod dns_record_manager;
use crate::dns_record_manager::{DnsRecordManager, GodaddyDnsRecordManager};
use anyhow::Context;
use env_logger;
use log::error;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    env_logger::init();

    let mut interval_timer = tokio::time::interval(Duration::from_secs(600));
    let mut manager = GodaddyDnsRecordManager::new().context("Create Godaddy Dns manager")?;

    loop {
        // Wait for the next interval tick
        interval_timer.tick().await;

        match manager.run().await {
            Ok(_) => {}
            Err(e) => error!("{}", e),
        };
    }
}
