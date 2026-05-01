//! Async helpers combining ENS resolution with verified ingest.

use async_trait::async_trait;

use crate::ens::IdentityResolver;
use crate::error::Result;
use crate::messaging::conversations::Conversations;
use crate::messaging::dispatch::{ingest_verified_inbound, MessageDispatcher, MessagingEvent};
use crate::messaging::envelope::WireEnvelope;
use crate::transport::PeerId;

#[async_trait]
pub trait InboundMessaging: Send + Sync {
    async fn ingest_inbound(
        &self,
        transport_peer_id: &PeerId,
        envelope: &WireEnvelope,
        conversations: &mut Conversations,
        dispatcher: &MessageDispatcher,
    ) -> Result<Vec<MessagingEvent>>;
}

#[async_trait]
impl<R: IdentityResolver + Send + Sync> InboundMessaging for R {
    async fn ingest_inbound(
        &self,
        transport_peer_id: &PeerId,
        envelope: &WireEnvelope,
        conversations: &mut Conversations,
        dispatcher: &MessageDispatcher,
    ) -> Result<Vec<MessagingEvent>> {
        let resolved = self.resolve_forward(envelope.from.trim()).await?;
        ingest_verified_inbound(transport_peer_id, &resolved, envelope, conversations, dispatcher)
    }
}
