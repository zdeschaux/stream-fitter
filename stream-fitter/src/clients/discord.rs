//! Implements a Discord client for relaying.
//!
//! Built on the serenity library for Discord API intercommunication.
use std::{option::Option, sync::Arc};

use futures::task::FutureObj;
use serde_derive::Deserialize;
use serenity::{
    async_trait,
    model::{channel::Message as SMessage, gateway::Ready, id::ChannelId},
    prelude::*,
};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tracing::{debug, error, info, instrument};

use crate::{
    clients::client::{Client as FitterClient, ClientTrait, Message},
    errors::{FitterErrorKind, FitterResult},
};

/// Handler struct for receiving and sending Discord messages.
struct DiscordHandler {
    ch_ids: Vec<ChannelId>,
    rx: Arc<Mutex<Receiver<Message>>>,
    tx: Sender<Message>,
    outer_tx: Vec<Sender<Message>>,
    isolate_channels: bool,
    forward_only: bool,
}

impl DiscordHandler {
    /// Creates a new handler for a channel.
    ///
    /// # Arguments
    ///
    /// * `channel_ids` - The Discord channel IDs.
    /// * `isolate_channels` - Don't forward to other channels.
    /// * `forward_only` - Forward to other clients, don't listen.
    fn new(channel_ids: Vec<u64>, isolate_channels: bool, forward_only: bool) -> Self {
        let (tx, rx) = channel(100);
        DiscordHandler {
            ch_ids: channel_ids.into_iter().map(ChannelId).collect(),
            rx: Arc::new(Mutex::new(rx)),
            tx,
            outer_tx: Vec::new(),
            isolate_channels,
            forward_only,
        }
    }

    /// Returns a copy of the handler's TX stream.
    fn get_stream(&self) -> Sender<Message> {
        self.tx.clone()
    }

    /// Adds a TX stream to send to on message receipt.
    ///
    /// # Arguments
    ///
    /// * `stream` - Another client's TX stream.
    fn add_stream(&mut self, stream: Sender<Message>) {
        self.outer_tx.push(stream);
    }
}

#[async_trait]
impl EventHandler for DiscordHandler {
    #[instrument(skip(self, ctx, msg))]
    async fn message(&self, ctx: Context, msg: SMessage) {
        // Only forward if it's not a bot message.
        if msg.author.bot {
            debug!("Bot, ignoring message");
            return;
        }

        // Only forward if it's coming from a channel we are handling.
        if let None = self.ch_ids.iter().find(|ch_id| ch_id == &&msg.channel_id) {
            debug!("Unrecognized channel, ignoring: {}", msg.channel_id);
            return;
        }

        let new_msg = Message::new(
            "Discord".to_string(),
            msg.channel_id.name(&ctx).await.unwrap(),
            msg.author.name,
            msg.content,
        );

        if !self.isolate_channels {
            // Forward message to other connected channels.
            for ch_id in &self.ch_ids {
                // Skip if same channel.
                if ch_id == &msg.channel_id {
                    continue;
                }

                if let Err(err) = ch_id.say(&ctx.http, new_msg.clone()).await {
                    error!("Error sending: {:?}", err);
                }
            }
        }

        // Forward message to all connected streams.
        for stream in &self.outer_tx {
            debug!("Sending message: {}", new_msg);
            if let Err(err) = stream.send(new_msg.clone()).await {
                error!("Error sending: {:?}", err);
            }
        }
    }

    #[instrument(skip(self, ctx, ready))]
    async fn ready(&self, ctx: Context, ready: Ready) {
        debug!("{} is connected!", ready.user.name);

        if !self.forward_only {
            // Start up the RX channel.
            let mut locked_rx = self.rx.lock().await;
            debug!("Lock acquired!");

            // Poll for new message.
            while let Some(msg) = locked_rx.recv().await {
                debug!("Received message! {}", msg);

                // Send received message to channels.
                for ch_id in &self.ch_ids {
                    if let Err(err) = ch_id.say(&ctx.http, msg.clone()).await {
                        error!("Error sending: {:?}", err);
                    }
                }
            }
        }
    }
}

/// Config struct for a Discord client.
#[derive(Deserialize)]
pub struct DiscordConfig {
    /// Bot's token.
    pub token: String,
    /// Vec of channel IDs to connect to.
    pub channel_ids: Vec<u64>,
    /// Don't forward between channels.
    pub isolate_channels: Option<bool>,
    /// Only forward to other clients, doesn't listen.
    pub forward_only: Option<bool>,
}

/// Discord client struct.
pub struct Discord {
    id: String,
    token: String,
    handler: Option<DiscordHandler>,
}

impl Discord {
    /// Build a Discord client.
    ///
    /// # Arguments
    ///
    /// * `id` - A client's unique ID.
    /// * `config` - The Discord config to build from.
    #[instrument(skip(config))]
    pub fn from_config(id: String, config: DiscordConfig) -> FitterResult<FitterClient> {
        info!("Initializing Discord client");
        Ok(Box::new(Discord {
            id,
            token: config.token,
            handler: Some(DiscordHandler::new(
                config.channel_ids,
                match config.isolate_channels {
                    Some(setting) => setting,
                    None => false,
                },
                match config.forward_only {
                    Some(setting) => setting,
                    None => false,
                },
            )),
        }))
    }
}

impl ClientTrait for Discord {
    type FutType = FutureObj<'static, FitterResult<()>>;

    fn get_name(&self) -> &str {
        "Discord"
    }

    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_stream(&self) -> FitterResult<Sender<Message>> {
        match &self.handler {
            Some(handler) => Ok(handler.get_stream()),
            None => Err(FitterErrorKind::GenericErr("No handler".to_string()).into()),
        }
    }

    fn add_stream(&mut self, stream: Sender<Message>) -> FitterResult<()> {
        match &mut self.handler {
            Some(handler) => {
                handler.add_stream(stream);
                Ok(())
            }
            None => Err(FitterErrorKind::InternalErr("No handler".to_string()).into()),
        }
    }

    #[instrument(skip(self))]
    fn run(&mut self) -> Self::FutType {
        info!("Starting Discord client {}", self.get_id());
        let handler = self.handler.take().unwrap();
        let token = self.token.clone();

        FutureObj::new(Box::new(async move {
            let mut client = Client::builder(token).event_handler(handler).await?;

            client.start().await?;
            Ok(())
        }))
    }
}
