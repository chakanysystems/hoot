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
    pub event_id: &'a str,
    pub status: bool,
    pub message: &'a str,
}

#[derive(Debug, Eq, PartialEq)]
pub enum RelayMessage<'a> {
    Event(&'a str, &'a str),
    OK(CommandResult<'a>),
    Eose(&'a str),
    Closed(&'a str, &'a str),
    Notice(&'a str),
    Auth(&'a str),
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
            let event_id = &msg[7..71];

            // Find the boolean value after the event_id
            let bool_start = 73;
            let bool_end = msg[bool_start..]
                .find(',')
                .map(|i| bool_start + i)
                .unwrap_or(msg.len() - 1);
            let bool_str = msg[bool_start..bool_end].trim();

            let status: bool = if bool_str == "true" {
                true
            } else if bool_str == "false" {
                false
            } else {
                return Err(error::Error::DecodeFailed);
            };

            // Extract the message field (everything after the boolean and comma)
            let message = if bool_end < msg.len() - 1 {
                let msg_start = bool_end + 1;
                // Exclude trailing ] and trim
                let msg_content = msg[msg_start..msg.len() - 1].trim();
                // Remove surrounding quotes if present
                if msg_content.len() >= 2
                    && msg_content.starts_with('"')
                    && msg_content.ends_with('"')
                {
                    &msg_content[1..msg_content.len() - 1]
                } else {
                    msg_content
                }
            } else {
                ""
            };

            return Ok(Self::ok(event_id, status, message));
        }

        // CLOSED (NIP-01)
        // Relay response format: ["CLOSED", <subscription_id>, <message>]
        if msg.len() >= 12 && &msg[0..=9] == "[\"CLOSED\"," {
            let mut start = 11;
            while let Some(&b' ') = msg.as_bytes().get(start) {
                start += 1;
            }
            if let Some(comma_index) = msg[start..].find(',') {
                let subid_end = start + comma_index;
                let subid = &msg[start..subid_end].trim().trim_matches('"');

                // Find start of message after subscription ID
                let msg_start = subid_end + 1;
                let mut msg_start = msg_start;
                while let Some(&b' ') = msg.as_bytes().get(msg_start) {
                    msg_start += 1;
                }

                // Message goes until end, minus closing bracket and quote
                let message = if msg_start < msg.len() - 1 {
                    let msg_content = &msg[msg_start..msg.len() - 1];
                    msg_content.trim().trim_matches('"')
                } else {
                    ""
                };

                return Ok(Self::Closed(subid, message));
            }
        }

        // AUTH (NIP-42)
        // Relay request format: ["AUTH", <challenge-string>]
        if msg.len() >= 10 && &msg[0..=6] == "[\"AUTH\"" {
            let start = if msg.as_bytes().get(8).copied() == Some(b' ') {
                9
            } else {
                8
            };
            let end = msg.len() - 1; // Remove trailing ]
            let challenge = msg[start..end].trim().trim_matches('"');
            return Ok(Self::Auth(challenge));
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
    Auth {
        event: Event,
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
            ClientMessage::Auth { event } => {
                let mut seq = serializer.serialize_seq(Some(2))?;
                seq.serialize_element("AUTH")?;
                seq.serialize_element(event)?;
                seq.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_auth_message() {
        // Test the exact message format from talon.quest
        let msg = r#"["AUTH","5538da53f3fb2cdb3aac6a6ca630a9a151c39a8c529396cfdc944a8d481ecf7f"]"#;
        let result = RelayMessage::from_json(msg);
        assert!(result.is_ok(), "Failed to parse AUTH message: {:?}", result);

        if let Ok(RelayMessage::Auth(challenge)) = result {
            assert_eq!(
                challenge,
                "5538da53f3fb2cdb3aac6a6ca630a9a151c39a8c529396cfdc944a8d481ecf7f"
            );
        } else {
            panic!("Expected Auth variant, got: {:?}", result);
        }
    }

    #[test]
    fn test_parse_auth_message_with_space() {
        // Test AUTH message with space after comma
        let msg = r#"["AUTH", "challenge-with-space"]"#;
        let result = RelayMessage::from_json(msg);
        assert!(
            result.is_ok(),
            "Failed to parse AUTH message with space: {:?}",
            result
        );

        if let Ok(RelayMessage::Auth(challenge)) = result {
            assert_eq!(challenge, "challenge-with-space");
        } else {
            panic!("Expected Auth variant, got: {:?}", result);
        }
    }

    #[test]
    fn test_parse_closed_message() {
        let msg =
            r#"["CLOSED","z6ThV92","auth-required: this subscription requires authentication"]"#;
        let result = RelayMessage::from_json(msg);
        assert!(
            result.is_ok(),
            "Failed to parse CLOSED message: {:?}",
            result
        );

        if let Ok(RelayMessage::Closed(sub_id, message)) = result {
            assert_eq!(sub_id, "z6ThV92");
            assert_eq!(
                message,
                "auth-required: this subscription requires authentication"
            );
        } else {
            panic!("Expected Closed variant, got: {:?}", result);
        }
    }

    #[test]
    fn test_parse_ok_message_success() {
        // Test successful OK message with 64-char event ID
        let event_id = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let msg = format!(r#"["OK","{}",true,""]"#, event_id);
        let result = RelayMessage::from_json(&msg);
        assert!(result.is_ok(), "Failed to parse OK message: {:?}", result);

        if let Ok(RelayMessage::OK(result)) = result {
            assert_eq!(result.event_id, event_id);
            assert_eq!(result.status, true);
            assert_eq!(result.message, "");
        } else {
            panic!("Expected OK variant, got: {:?}", result);
        }
    }

    #[test]
    fn test_parse_ok_message_failure() {
        // Test failed OK message with error message
        let event_id = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let msg = format!(r#"["OK","{}",false,"rate-limited: slow down"]"#, event_id);
        let result = RelayMessage::from_json(&msg);
        assert!(result.is_ok(), "Failed to parse OK message: {:?}", result);

        if let Ok(RelayMessage::OK(result)) = result {
            assert_eq!(result.event_id, event_id);
            assert_eq!(result.status, false);
            assert_eq!(result.message, "rate-limited: slow down");
        } else {
            panic!("Expected OK variant, got: {:?}", result);
        }
    }

    #[test]
    fn test_parse_ok_message_auth_required() {
        // Test OK message with auth-required prefix (critical for NIP-42 flow)
        let event_id = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let msg = format!(
            r#"["OK","{}",false,"auth-required: please authenticate"]"#,
            event_id
        );
        let result = RelayMessage::from_json(&msg);
        assert!(result.is_ok(), "Failed to parse OK message: {:?}", result);

        if let Ok(RelayMessage::OK(result)) = result {
            assert_eq!(result.event_id, event_id);
            assert_eq!(result.status, false);
            assert_eq!(result.message, "auth-required: please authenticate");
            assert!(
                result.message.starts_with("auth-required:"),
                "Should detect auth-required prefix"
            );
        } else {
            panic!("Expected OK variant, got: {:?}", result);
        }
    }

    #[test]
    fn test_parse_ok_message_no_message() {
        // Test OK message without message field
        let event_id = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let msg = format!(r#"["OK","{}",true]"#, event_id);
        let result = RelayMessage::from_json(&msg);
        assert!(
            result.is_ok(),
            "Failed to parse OK message without message: {:?}",
            result
        );

        if let Ok(RelayMessage::OK(result)) = result {
            assert_eq!(result.event_id, event_id);
            assert_eq!(result.status, true);
            assert_eq!(result.message, "");
        } else {
            panic!("Expected OK variant, got: {:?}", result);
        }
    }

    #[test]
    fn test_command_result_fields() {
        // Test that CommandResult fields are accessible
        let result = CommandResult {
            event_id: "test-id",
            status: true,
            message: "test message",
        };

        assert_eq!(result.event_id, "test-id");
        assert_eq!(result.status, true);
        assert_eq!(result.message, "test message");
    }

    #[test]
    fn test_client_message_auth_serialization() {
        // Test that ClientMessage::Auth serializes correctly
        use nostr::Keys;

        let keys = Keys::generate();
        let relay_url = nostr::RelayUrl::parse("wss://relay.example.com").unwrap();
        let event = nostr::EventBuilder::auth("test-challenge", relay_url)
            .sign_with_keys(&keys)
            .expect("Failed to create auth event");

        let client_msg = ClientMessage::Auth {
            event: event.clone(),
        };
        let json = serde_json::to_string(&client_msg).expect("Failed to serialize");

        // Verify it starts with ["AUTH",
        assert!(json.starts_with("[\"AUTH\","));
        // Verify the event is included
        assert!(json.contains(&event.id.to_string()));
    }
}
