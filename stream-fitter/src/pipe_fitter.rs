//! The central manager to load and interconnect clients.
use std::{collections::HashMap, sync::Arc, vec::Vec};

use nanoid::nanoid;
use serde_derive::Deserialize;
use tokio::sync::{mpsc::Sender, Mutex};
use tracing::{debug, error, info, instrument};

use crate::{
    clients::client::{Client, ClientConfig, Message},
    errors::FitterResult,
};

/// Configuration for pipe manager containing the configs of streams we want to connect.
#[derive(Deserialize)]
pub struct PipeFitterConfig {
    stream_configs: Vec<ClientConfig>,
}

/// Alias for the client type used by the stream manager.
type PipeFitterClient = Arc<Mutex<Client>>;

/// Stream manager struct.
pub struct PipeFitter {
    clients: Vec<PipeFitterClient>,
}

impl PipeFitter {
    /// Build a stream manager from a config.
    ///
    /// # Arguments
    ///
    /// * `config` - A stream manager config to load.
    #[instrument(skip(config))]
    pub fn from_config(config: PipeFitterConfig) -> FitterResult<Self> {
        info!("Instantiating PipeFitter");

        // Build clients
        let mut clients = config
            .stream_configs
            .into_iter()
            .map(|stream_config| ClientConfig::from_config(nanoid!(), stream_config))
            .collect::<FitterResult<Vec<Client>>>()?;

        // Need to collect clients' tx channels from each other
        let mut client_map = clients
            .iter()
            .map(|client| {
                debug!("Mapping client: {} {}", client.get_name(), client.get_id());
                (
                    client.get_id().to_string(),
                    clients
                        .iter()
                        .filter(|other_client| other_client.get_id() != client.get_id())
                        .map(|other_client| {
                            debug!(
                                "Adding client {} {} to client {} {}",
                                other_client.get_name(),
                                other_client.get_id(),
                                client.get_name(),
                                client.get_id()
                            );
                            other_client.get_stream().unwrap()
                        })
                        .collect::<Vec<Sender<Message>>>(),
                )
            })
            .collect::<HashMap<String, Vec<Sender<Message>>>>();

        // Add streams and construct stream manager clients
        let pipe_fitter_clients = clients
            .drain(..)
            .map(|mut client| {
                if let Some(streams) = client_map.get_mut(client.get_id()) {
                    streams
                        .drain(..)
                        .try_for_each(|stream| client.add_stream(stream))
                        .unwrap()
                };
                Arc::new(Mutex::new(client))
            })
            .collect();

        Ok(PipeFitter {
            clients: pipe_fitter_clients,
        })
    }

    /// Run the stream manager.
    #[instrument(skip(self))]
    pub fn run(&mut self) -> FitterResult<()> {
        info!("Running PipeFitter");
        let mut clients = self.clients.drain(..);

        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                loop {
                    let client = match clients.next() {
                        Some(client) => Arc::clone(&client),
                        None => continue,
                    };
                    tokio::spawn(async move {
                        match client.lock().await.run().await {
                            Ok(_) => (),
                            Err(err) => {
                                error!("Stream error: {:?}", err);
                            }
                        }
                    });
                }
            });
        Ok(())
    }
}
