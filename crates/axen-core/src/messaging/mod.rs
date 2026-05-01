//! Messaging: envelopes, EIP-712 verification hooks, handlers, and ephemeral conversations.

mod conversations;
mod dispatch;
mod envelope;
mod identity_wire;
mod inbound;

pub use conversations::{ChatMessage, Conversations, MessageState};
pub use dispatch::{
    ingest_verified_inbound, verify_transport_matches_ens, verify_wallet_signature, ChatTextV1Handler,
    DispatchContext, MessageDispatcher, MessageHandler, MessagingEvent,
};
pub use envelope::{chat_text_v1_body_json, WireEnvelope};
pub use identity_wire::ResolvedIdentityWire;
pub use inbound::InboundMessaging;
