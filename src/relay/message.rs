use crate::error;
use ewebsock::{WsEvent, WsMessage};
use nostr::types::Filter;
use nostr::Event;
use serde::de::{SeqAccess, Visitor};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{self};

#[derive(Debug, Eq, PartialEq)]
pub struct CommandResult<'a> {
    event_id: &'a str,
    status: bool,
    message: &'a str,
}

#[derive(Debug, Eq, PartialEq)]
pub enum RelayMessage<'a> {
    Event(&'a str, &'a str),
    OK(CommandResult<'a>),
    Eose(&'a str),
    Closed(&'a str, &'a str),
    Notice(&'a str),
}

#[derive(Debug)]
pub enum RelayEvent<'a> {
    Opened,
    Closed,
    Other(&'a WsMessage),
    Error(error::Error),
    Message(RelayMessage<'a>),
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

impl<'a> RelayMessage<'a> {
    pub fn eose(subid: &'a str) -> Self {
        RelayMessage::Eose(subid)
    }

    pub fn notice(msg: &'a str) -> Self {
        RelayMessage::Notice(msg)
    }

    pub fn ok(event_id: &'a str, status: bool, message: &'a str) -> Self {
        RelayMessage::OK(CommandResult {
            event_id,
            status,
            message,
        })
    }

    pub fn event(ev: &'a str, sub_id: &'a str) -> Self {
        RelayMessage::Event(sub_id, ev)
    }

    pub fn from_json(msg: &'a str) -> error::Result<RelayMessage<'a>> {
        if msg.is_empty() {
            return Err(error::Error::Empty);
        }

        // Notice
        // Relay response format: ["NOTICE", <message>]
        if msg.len() >= 12 && &msg[0..=9] == "[\"NOTICE\"," {
            // TODO: there could be more than one space, whatever
            let start = if msg.as_bytes().get(10).copied() == Some(b' ') {
                12
            } else {
                11
            };
            let end = msg.len() - 2;
            return Ok(Self::notice(&msg[start..end]));
        }

        // Event
        // Relay response format: ["EVENT", <subscription id>, <event JSON>]
        if &msg[0..=7] == "[\"EVENT\"" {
            let mut start = 9;
            while let Some(&b' ') = msg.as_bytes().get(start) {
                start += 1; // Move past optional spaces
            }
            if let Some(comma_index) = msg[start..].find(',') {
                let subid_end = start + comma_index;
                let subid = &msg[start..subid_end].trim().trim_matches('"');

                // Find start of event JSON after subscription ID
                let event_start = subid_end + 1;
                let mut event_start = event_start;
                while let Some(&b' ') = msg.as_bytes().get(event_start) {
                    event_start += 1;
                }

                // Event JSON goes until end, minus closing bracket
                let event_json = &msg[event_start..msg.len() - 1];

                return Ok(Self::event(event_json, subid));
            } else {
                return Ok(Self::event("{}", "fixme")); // Empty event JSON if parsing fails
            }
        }

        // EOSE (NIP-15)
        // Relay response format: ["EOSE", <subscription_id>]
        if &msg[0..=7] == "[\"EOSE\"," {
            let start = if msg.as_bytes().get(8).copied() == Some(b' ') {
                10
            } else {
                9
            };
            let end = msg.len() - 2;
            return Ok(Self::eose(&msg[start..end]));
        }

        // OK (NIP-20)
        // Relay response format: ["OK",<event_id>, <true|false>, <message>]
        if &msg[0..=5] == "[\"OK\"," && msg.len() >= 78 {
            // TODO: fix this
            let event_id = &msg[7..71];
            let booly = &msg[73..77];
            let status: bool = if booly == "true" {
                true
            } else if booly == "false" {
                false
            } else {
                return Err(error::Error::DecodeFailed);
            };

            return Ok(Self::ok(event_id, status, "fixme"));
        }

        Err(error::Error::DecodeFailed)
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
