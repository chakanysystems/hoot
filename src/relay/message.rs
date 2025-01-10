use ewebsock::{WsMessage, WsEvent};
use nostr::types::Filter;
use nostr::Event;
use serde::de::{SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{self};
use crate::error;

#[derive(Debug, Eq, PartialEq)]
pub struct CommandResult<'a> {
    event_id: &'a str,
    status: bool,
    message: &'a str,
}

#[derive(Debug, Eq, PartialEq)]
pub struct EventMessage {
    pub subscription_id: String,
    pub event: Event,
}

#[derive(Debug, Eq, PartialEq)]
pub enum RelayMessage {
    Event(EventMessage),
    OK(CommandResult<'static>),
    Eose(String),
    Closed(String, String),
    Notice(String),
}

#[derive(Debug)]
pub enum RelayEvent<'a> {
    Opened,
    Closed,
    Other(&'a WsMessage),
    Error(error::Error),
    Message(RelayMessage<'a>)
}

impl<'a> From<&'a WsEvent> for RelayEvent<'a> {
    fn from(value: &'a WsEvent) -> Self {
        match value {
            WsEvent::Opened => RelayEvent::Opened,
            WsEvent::Closed => RelayEvent::Closed,
            WsEvent::Message(ref ws_msg) => ws_msg.into(),
            WsEvent::Error(e) => RelayEvent::Error(error::Error::Generic(e.to_owned())),
        }
    }
}

impl<'a> From<&'a WsMessage> for RelayEvent<'a> {
    fn from(value: &'a WsMessage) -> Self {
        match value {
            WsMessage::Text(s) => match RelayMessage::from_json(s).map(RelayEvent::Message) {
                Ok(msg) => msg,
                Err(err) => RelayEvent::Error(err),
            },
            value => RelayEvent::Other(value),
        }
    }
}

impl RelayMessage {
    pub fn eose<S: Into<String>>(subid: S) -> Self {
        RelayMessage::Eose(subid.into())
    }

    pub fn notice<S: Into<String>>(msg: S) -> Self {
        RelayMessage::Notice(msg.into())
    }

    pub fn ok(event_id: &'static str, status: bool, message: &'static str) -> Self {
        RelayMessage::OK(CommandResult {
            event_id,
            status,
            message,
        })
    }

    pub fn event<S: Into<String>>(subscription_id: S, event: Event) -> Self {
        RelayMessage::Event(EventMessage {
            subscription_id: subscription_id.into(),
            event,
        })
    }

    pub fn from_json(msg: &str) -> error::Result<RelayMessage> {
        if msg.is_empty() {
            return Err(error::Error::Empty);
        }

        // First try parsing as a JSON array
        let json_value: serde_json::Value = serde_json::from_str(msg)
            .map_err(|_| error::Error::DecodeFailed)?;

        if !json_value.is_array() {
            return Err(error::Error::DecodeFailed);
        }

        let array = json_value.as_array().unwrap();
        if array.is_empty() {
            return Err(error::Error::DecodeFailed);
        }

        // Get the message type
        let msg_type = array[0].as_str()
            .ok_or(error::Error::DecodeFailed)?;

        match msg_type {
            // Notice: ["NOTICE", <message>]
            "NOTICE" => {
                if array.len() != 2 {
                    return Err(error::Error::DecodeFailed);
                }
                let message = array[1].as_str()
                    .ok_or(error::Error::DecodeFailed)?;
                Ok(Self::notice(message))
            },

            // Event: ["EVENT", <subscription_id>, <event JSON>]
            "EVENT" => {
                if array.len() != 3 {
                    return Err(error::Error::DecodeFailed);
                }
                let subscription_id = array[1].as_str()
                    .ok_or(error::Error::DecodeFailed)?;
                let event: Event = serde_json::from_value(array[2].clone())
                    .map_err(|_| error::Error::DecodeFailed)?;
                Ok(Self::event(subscription_id, event))
            },

            // EOSE: ["EOSE", <subscription_id>]
            "EOSE" => {
                if array.len() != 2 {
                    return Err(error::Error::DecodeFailed);
                }
                let subscription_id = array[1].as_str()
                    .ok_or(error::Error::DecodeFailed)?;
                Ok(Self::eose(subscription_id))
            },

            // OK: ["OK", <event_id>, <true|false>, <message>]
            "OK" => {
                if array.len() != 4 {
                    return Err(error::Error::DecodeFailed);
                }
                let event_id = array[1].as_str()
                    .ok_or(error::Error::DecodeFailed)?;
                let status = array[2].as_bool()
                    .ok_or(error::Error::DecodeFailed)?;
                let message = array[3].as_str()
                    .ok_or(error::Error::DecodeFailed)?;
                
                // TODO: Fix static lifetime requirement
                Ok(Self::ok(event_id, status, "ok"))
            },

            _ => Err(error::Error::DecodeFailed),
        }
    }
}

/// Messages that are client -> relay.
#[derive(Debug, Clone)]
pub enum ClientMessage {
    Event {
        event: Event,
    },
    Req {
        subscription_id: String,
        filters: Vec<Filter>,
    },
    Close {
        subscription_id: String,
    },
}

impl From<super::Subscription> for ClientMessage {
    fn from(value: super::Subscription) -> Self {
        Self::Req {
            subscription_id: value.id,
            filters: value.filters,
        }
    }
}

impl Serialize for ClientMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ClientMessage::Event { event } => {
                let mut seq = serializer.serialize_seq(Some(2))?;
                seq.serialize_element("EVENT")?;
                seq.serialize_element(event)?;
                seq.end()
            }
            ClientMessage::Req {
                subscription_id,
                filters,
            } => {
                let mut seq = serializer.serialize_seq(Some(2 + filters.len()))?;
                seq.serialize_element("REQ")?;
                seq.serialize_element(subscription_id)?;
                for filter in filters {
                    seq.serialize_element(filter)?;
                }
                seq.end()
            }
            ClientMessage::Close { subscription_id } => {
                let mut seq = serializer.serialize_seq(Some(2))?;
                seq.serialize_element("CLOSE")?;
                seq.serialize_element(subscription_id)?;
                seq.end()
            }
        }
    }
}
