use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, trace, warn, info};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use crate::stream::Stream;

mod stream;

const FRAME_TIMEOUT_SEC: u64 = 30;

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Config {
    /// Snapshot interval in seconds.
    snapshot_interval: u32,

    #[serde(rename = "camera")]
    cameras: Vec<CameraConfig>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct CameraConfig {
    token: String,
    url: String,
    username: Option<String>,
    password: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let args: Vec<String> = std::env::args().collect();
    let default = String::from("config.toml");
    let config_path = args.get(1).unwrap_or(&default);

    info!("loading config from {}", config_path);

    let config = read_config(config_path)
        .await
        .context("error reading config")?;

    if config.cameras.is_empty() {
        bail!("no cameras specified");
    }

    let mut streams = config
        .cameras
        .iter()
        .map(|config| {
            let stream = Stream::new(config).context("error constructing stream")?;

            Ok((&config.token, stream))
        })
        .collect::<Result<HashMap<_, _>>>()?;

    let client = reqwest::Client::builder().build()?;

    loop {
        debug!("polling for frames from {} cameras", streams.len());
        for (&token, stream) in streams.iter_mut() {
            match tokio::time::timeout(Duration::from_secs(FRAME_TIMEOUT_SEC), stream.next()).await
            {
                Ok(Ok(frame)) => {
                    debug!("uploading image for camera {}", token);
                    let result = client
                        .put("https://webcam.connect.prusa3d.com/c/snapshot")
                        .header("content-type", "image/jpg")
                        .header("Fingerprint", token)
                        .header("Token", token)
                        .body(frame)
                        .send()
                        .await
                        .and_then(|r| r.error_for_status());

                    if let Err(e) = result {
                        error!(err=?e, "error uploading frame: {:?}", e)
                    }
                }
                Ok(Err(e)) => {
                    error!(err=?e, "error retrieving frame: {:?}", e)
                }
                Err(e) => {
                    warn!("timeout waiting for frame after {}", e)
                }
            }
        }

        trace!("sleeping for {}s", config.snapshot_interval);
        tokio::time::sleep(Duration::from_secs(config.snapshot_interval as u64)).await;
    }
}

fn init_logging() {
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

async fn read_config<P: AsRef<Path>>(path: P) -> Result<Config> {
    let mut file = File::open(path).await?;

    let mut contents = String::new();
    file.read_to_string(&mut contents).await?;

    Ok(toml::from_str(&contents)?)
}
