//! Client trait and utilities definitions.
use std::fmt::{Display, Formatter, Result};

use futures::{future::Future, task::FutureObj};
use serde_derive::{Deserialize, Serialize};
use tokio::sync::mpsc::Sender;

use crate::{
    clients::{discord, twitch},
    errors::FitterResult,
};

/// Message type to use for intercommunication between streams.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Message {
    client: String,
    channel: String,
    author: String,
    content: String,
}

impl Message {
    /// Create a new message.
    ///
    /// # Arguments
    ///
    /// * `client` - The client generating the message.
    /// * `channel` - The message's channel.
    /// * `author` - The message's author.
    /// * `content` - The message's content.
    pub fn new(client: String, channel: String, author: String, content: String) -> Message {
        Message {
            client,
            channel,
            author,
            content,
        }
    }
}

impl Display for Message {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(
            f,
            "[{}: {}] [{}] {}",
            self.client, self.channel, self.author, self.content
        )
    }
}

/// Client trait to implement for chat clients.
pub trait ClientTrait {
    /// Future return type when running.
    type FutType: Future<Output = FitterResult<()>>;

    /// Gets the name of the client.
    fn get_name(&self) -> &str;

    /// Gets the unique ID of the client.
    fn get_id(&self) -> &str;

    /// Gets a copy of the TX stream for this client.
    fn get_stream(&self) -> FitterResult<Sender<Message>>;

    /// Adds a TX stream to send to for this client.
    ///
    /// # Arguments
    ///
    /// * `stream` - The other client's TX stream.
    fn add_stream(&mut self, stream: Sender<Message>) -> FitterResult<()>;

    /// Run the client's main loop.
    fn run(&mut self) -> Self::FutType;
}

/// Client type alias to implement for.
pub type Client = Box<dyn ClientTrait<FutType = FutureObj<'static, FitterResult<()>>> + Send>;

/// Client configuration enum for deserializing.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum ClientConfig {
    #[serde(rename = "discord")]
    DiscordConfig(discord::DiscordConfig),
    #[serde(rename = "twitch")]
    TwitchConfig(twitch::TwitchConfig),
}

impl ClientConfig {
    /// Build a client from a config.
    ///
    /// # Arguments
    ///
    /// * `id` - A client's unique ID.
    /// * `config` - A client's config.
    pub fn from_config(id: String, config: ClientConfig) -> FitterResult<Client> {
        match config {
            ClientConfig::DiscordConfig(cfg) => discord::Discord::from_config(id, cfg),
            ClientConfig::TwitchConfig(cfg) => twitch::Twitch::from_config(id, cfg),
        }
    }
}
