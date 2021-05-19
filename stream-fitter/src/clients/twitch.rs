//! Implements a Twitch client for relaying.
//!
//! Built on the twitchchat library for Twitch API intercommunication.
use std::{option::Option, sync::Arc};

use futures::task::FutureObj;
use serde_derive::Deserialize;
use tokio::sync::{
    mpsc::{channel, Receiver, Sender, UnboundedReceiver},
    Mutex,
};
use tracing::{debug, error, info, instrument};
use twitch_irc::{
    login::StaticLoginCredentials, message::ServerMessage, ClientConfig, TCPTransport,
    TwitchIRCClient,
};

use crate::{
    clients::client::{Client as FitterClient, ClientTrait, Message},
    errors::FitterResult,
};

/// Loop to broadcast received Twitch messages.
///
/// # Arguments
///
/// * `inner_rx` - The RX channel of the Twitch chat client.
/// * `client_name` - The clients name to ignore messages from.
/// * `channels` - The channels to forward messages from.
/// * `client` - The Twitch client to broadcast to.
/// * `outer_tx` - The TX channels of other clients.
/// * `isolate_channels` - Don't forward to other channels.
#[instrument(skip(inner_rx, outer_tx))]
async fn external_message_loop(
    mut inner_rx: UnboundedReceiver<ServerMessage>,
    client_name: String,
    channels: Vec<String>,
    client: TwitchIRCClient<TCPTransport, StaticLoginCredentials>,
    outer_tx: Vec<Sender<Message>>,
    isolate_channels: bool,
) {
    while let Some(msg) = inner_rx.recv().await {
        if let ServerMessage::Privmsg(msg) = msg {
            // Only forward if it's not a bot message.
            if msg.sender.login == client_name {
                debug!("Bot, ignoring message");
                continue;
            }

            // Only forward if it's coming from the channel we are handling.
            if let None = channels
                .iter()
                .find(|channel| channel == &&msg.channel_login)
            {
                debug!("Unrecognized channel, ignoring: {}", msg.channel_login);
                continue;
            }

            let new_msg = Message::new(
                "Twitch".to_string(),
                msg.channel_login.clone(),
                msg.sender.name,
                msg.message_text,
            );

            if !isolate_channels {
                // Forward message to other connected channels.
                for channel in &channels {
                    // Skip if same channel.
                    if channel == &msg.channel_login {
                        continue;
                    }

                    if let Err(err) = client.privmsg(channel.clone(), new_msg.to_string()).await {
                        error!("Error sending: {:?}", err);
                    }
                }
            }

            // Forward message to all connected streams.
            for stream in &outer_tx {
                debug!("Sending message: {}", new_msg);
                if let Err(err) = stream.send(new_msg.clone()).await {
                    error!("Error sending: {:?}", err);
                }
            }
        }
    }
}

/// Loop to broadcast to Twitch received internal messages.
///
/// # Arguments
///
/// * `rx` - The RX channel for the client.
/// * `client` - The Twitch client to broadcast to.
/// * `channels` - The channels to forward messages to.
#[instrument(skip(rx, client))]
async fn internal_message_loop(
    rx: Arc<Mutex<Receiver<Message>>>,
    client: TwitchIRCClient<TCPTransport, StaticLoginCredentials>,
    channels: Vec<String>,
) {
    let mut locked_rx = rx.lock().await;
    debug!("Lock acquired!");

    // Poll for new message.
    while let Some(msg) = locked_rx.recv().await {
        debug!("Received message! {}", msg);

        // Send received message to channels.
        for channel in &channels {
            if let Err(err) = client.privmsg(channel.clone(), msg.to_string()).await {
                error!("Error sending: {:?}", err);
            };
        }
    }
}

/// Config struct for a Twitch client.
#[derive(Deserialize)]
pub struct TwitchConfig {
    /// Bot's token.
    pub token: String,
    /// Bot's name.
    pub name: String,
    /// Vec of channels to connect to.
    pub channels: Vec<String>,
    /// Don't forward between channels.
    pub isolate_channels: Option<bool>,
    /// Only forward to other clients, doesn't listen.
    pub forward_only: Option<bool>,
}

/// Twitch client struct.
pub struct Twitch {
    id: String,
    user_config: Option<ClientConfig<StaticLoginCredentials>>,
    channels: Vec<String>,
    rx: Arc<Mutex<Receiver<Message>>>,
    tx: Sender<Message>,
    outer_tx: Vec<Sender<Message>>,
    isolate_channels: bool,
    forward_only: bool,
}

impl Twitch {
    /// Build a Twitch client.
    ///
    /// # Arguments
    ///
    /// * `id` - A client's unique ID.
    /// * `config` - The Twitch config to build from.
    #[instrument(skip(config))]
    pub fn from_config(id: String, config: TwitchConfig) -> FitterResult<FitterClient> {
        info!("Initializing Twitch client");
        let (tx, rx) = channel(100);
        Ok(Box::new(Twitch {
            id,
            user_config: Some(ClientConfig::new_simple(StaticLoginCredentials::new(
                config.name,
                Some(config.token),
            ))),
            channels: config.channels,
            rx: Arc::new(Mutex::new(rx)),
            tx,
            outer_tx: Vec::new(),
            isolate_channels: match config.isolate_channels {
                Some(setting) => setting,
                None => false,
            },
            forward_only: match config.forward_only {
                Some(setting) => setting,
                None => false,
            },
        }))
    }
}

impl ClientTrait for Twitch {
    type FutType = FutureObj<'static, FitterResult<()>>;

    fn get_name(&self) -> &str {
        "Twitch"
    }

    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_stream(&self) -> FitterResult<Sender<Message>> {
        Ok(self.tx.clone())
    }

    fn add_stream(&mut self, stream: Sender<Message>) -> FitterResult<()> {
        self.outer_tx.push(stream);
        Ok(())
    }

    #[instrument(skip(self))]
    fn run(&mut self) -> Self::FutType {
        info!("Starting Twitch client {}", self.get_id());
        let user_config = self.user_config.take().unwrap();
        let name = user_config.login_credentials.credentials.login.clone();
        let channels = self.channels.clone();
        let rx = Arc::clone(&self.rx);
        let outer_tx = self.outer_tx.drain(..).collect::<Vec<Sender<Message>>>();
        let isolate_channels = self.isolate_channels;
        let forward_only = self.forward_only;

        FutureObj::new(Box::new(async move {
            let (inner_rx, client) =
                TwitchIRCClient::<TCPTransport, StaticLoginCredentials>::new(user_config);

            debug!("{} is connected!", name);

            // Spawn thread to handle incoming messages from Twitch.
            let send_channels = channels.clone();
            let forward_client = client.clone();
            let join_send = tokio::spawn(async move {
                external_message_loop(
                    inner_rx,
                    name,
                    send_channels,
                    forward_client,
                    outer_tx,
                    isolate_channels,
                )
                .await;
            });

            // Join the specified channel.
            channels
                .iter()
                .for_each(|channel| client.join(channel.clone()));

            if !forward_only {
                // Handle incoming messages from other clients.
                let join_read = tokio::spawn(async move {
                    internal_message_loop(rx, client, channels).await;
                });
                join_read.await?;
            }

            join_send.await?;
            Ok(())
        }))
    }
}
