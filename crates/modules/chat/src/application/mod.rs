//! Receives chat messages from Twitch or the simulator and hands them on while the module is enabled.

use std::sync::Arc;

use toolkit_core::{
    events::{ChatMessage, Source},
    modules::{self, Modules},
};

/// Hands a message to the overlays; `infrastructure` publishes it on the bus.
type Publish = Box<dyn Fn(ChatMessage, Source) + Send + Sync>;

pub struct ChatService {
    modules: Arc<Modules>,
    publish: Publish,
}

impl ChatService {
    pub(crate) fn with_publisher(
        modules: Arc<Modules>,
        publish: impl Fn(ChatMessage, Source) + Send + Sync + 'static,
    ) -> Self {
        Self {
            modules,
            publish: Box::new(publish),
        }
    }

    /// Dropped while the chat module is disabled.
    pub fn receive(&self, message: ChatMessage, source: Source) {
        if !self.modules.is_enabled(modules::CHAT) {
            return;
        }
        (self.publish)(message, source);
    }
}
