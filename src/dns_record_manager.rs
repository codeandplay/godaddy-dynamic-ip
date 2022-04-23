use log::{error, info};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;

#[derive(Debug)]
pub struct GodaddyConfig {
    pub api_key: String,
    pub api_secret: String,
    pub base_path: String,
    pub record_name: String,
}

#[derive(thiserror::Error, Debug)]
pub enum GodaddyConfigError {
    #[error("Environmental variables are not set: API_KEY, API_SECRET, BASE_PATH, RECORD_NAME")]
    MissingEnvironmentVariables,
}

impl GodaddyConfig {
    pub fn load() -> Result<Self, GodaddyConfigError> {
        match (
            env::var("API_KEY"),
            env::var("API_SECRET"),
            env::var("BASE_PATH"),
            env::var("RECORD_NAME"),
        ) {
            (Ok(k), Ok(s), Ok(p), Ok(n)) => Ok(Self {
                api_secret: s.to_string(),
                api_key: k.to_string(),
                base_path: p.to_string(),
                record_name: n.to_string(),
            }),
            (_, _, _, _) => Err(GodaddyConfigError::MissingEnvironmentVariables),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DnsRecord {
    pub data: String,
}

#[derive(thiserror::Error, Debug)]
pub enum DnsRecordManagerError {
    #[error("Unable to get public IP")]
    UnableToGetPublicIp,
    #[error("Unable to send request.")]
    RequestFail(anyhow::Error),
    #[error("Fail to parse reponse: {0}")]
    FailToParseResponse(anyhow::Error),
    #[error("Error response when update record: {0}")]
    UpdateRecordError(anyhow::Error),
    #[error("Unexpected error: {0}")]
    Unexpected(String),
}

#[async_trait::async_trait]
pub trait DnsRecordManager {
    async fn get_current_public_ip() -> Result<String, DnsRecordManagerError>;

    async fn get_arecord_detail(&self) -> Result<DnsRecord, DnsRecordManagerError>;

    async fn update_arecord_detail(&self, new_ip: &str) -> Result<(), DnsRecordManagerError>;

    async fn run(&mut self) -> Result<(), DnsRecordManagerError>;
}

pub struct GodaddyDnsRecordManager {
    config: GodaddyConfig,
    client: Client,
    current_record: Option<DnsRecord>,
}

impl GodaddyDnsRecordManager {
    pub fn new() -> Result<Self, GodaddyConfigError> {
        let config = GodaddyConfig::load()?;
        let client = Client::new();
        Ok(Self {
            config,
            client,
            current_record: None,
        })
    }
}

#[async_trait::async_trait]
impl DnsRecordManager for GodaddyDnsRecordManager {
    async fn get_current_public_ip() -> Result<String, DnsRecordManagerError> {
        if let Some(ip) = public_ip::addr().await {
            Ok(ip.to_string())
        } else {
            Err(DnsRecordManagerError::UnableToGetPublicIp)
        }
    }

    async fn get_arecord_detail(&self) -> Result<DnsRecord, DnsRecordManagerError> {
        let req = self
            .client
            .get(format!(
                "{}/domains/{}/records/A/%40",
                self.config.base_path, self.config.record_name
            ))
            .header(
                "Authorization",
                format!("sso-key {}:{}", self.config.api_key, self.config.api_secret),
            );

        let response = req
            .send()
            .await
            .map_err(|e| DnsRecordManagerError::RequestFail(anyhow::anyhow!(e)))?;

        if !response.status().is_success() {
            return Err(DnsRecordManagerError::Unexpected(format!(
                "Error response from Godaddy: {}",
                response
                    .text()
                    .await
                    .map_err(|_e| DnsRecordManagerError::Unexpected(
                        "Reponse is not text".to_string()
                    ))?
            )));
        }

        let dns_records = response
            .json::<Vec<DnsRecord>>()
            .await
            .map_err(|e| DnsRecordManagerError::FailToParseResponse(anyhow::anyhow!(e)))?;

        let dns_record = dns_records.into_iter().next().ok_or_else(|| {
            DnsRecordManagerError::Unexpected("Cannot find DNS record.".to_string())
        })?;

        Ok(dns_record)
    }

    async fn update_arecord_detail(&self, new_ip: &str) -> Result<(), DnsRecordManagerError> {
        let req = self
            .client
            .put(format!(
                "{}/domains/{}/records/A/%40",
                self.config.base_path, self.config.record_name
            ))
            .header(
                "Authorization",
                format!("sso-key {}:{}", self.config.api_key, self.config.api_secret),
            )
            .header("content-type", "application/json")
            .body(
                json!([{
                    "data": new_ip,
                    "ttl": 600
                }])
                .to_string(),
            );

        println!("req is: {:?}", req);

        let response = req
            .send()
            .await
            .map_err(|e| DnsRecordManagerError::RequestFail(anyhow::anyhow!(e)))?;
        if !response.status().is_success() {
            println!("data is {:?}", response.text().await.unwrap());
            return Err(DnsRecordManagerError::UpdateRecordError(anyhow::anyhow!(
                "{:?}", ""
            )));
        }
        Ok(())
    }

    async fn run(&mut self) -> Result<(), DnsRecordManagerError> {
        // check if we have dns record detail yet.
        if self.current_record.is_none() {
            let record = self.get_arecord_detail().await?;
            self.current_record = Some(record);
        }

        // check current public ip
        let current_public_ip = Self::get_current_public_ip().await?;
        let current_record = self.current_record.as_ref().ok_or_else(|| {
            DnsRecordManagerError::Unexpected("Current record should be set".to_string())
        })?;

        if !current_public_ip.eq(&current_record.data) {
            info!("DNS record out of date");
            self.update_arecord_detail(&current_public_ip).await?;
            self.current_record
                .as_mut()
                .ok_or_else(|| {
                    DnsRecordManagerError::Unexpected("Current record should be set".to_string())
                })?
                .data = current_public_ip.to_owned();

            info!("Updated DNS record A record to {}", current_public_ip);
        } else {
            info!("DNS record already update to date, does not need to update.")
        }

        Ok(())
    }
}
