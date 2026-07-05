use crate::error;
use nostr::types::Filter;
use nostr::Event;
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};

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

fn split_relay_array_fields(msg: &str) -> Option<Vec<&str>> {
    if !(msg.starts_with('[') && msg.ends_with(']')) {
        return None;
    }

    let inner = &msg[1..msg.len() - 1];
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }

    let mut fields = Vec::new();
    let mut field_start = 0;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, byte) in inner.bytes().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => depth += 1,
            b']' | b'}' => depth = depth.checked_sub(1)?,
            b',' if depth == 0 => {
                fields.push(inner[field_start..index].trim());
                field_start = index + 1;
            }
            _ => {}
        }
    }

    if in_string || escaped || depth != 0 {
        return None;
    }

    fields.push(inner[field_start..].trim());
    Some(fields)
}

fn string_field(field: &str) -> Option<&str> {
    let field = field.trim();
    if field.len() >= 2 && field.starts_with('"') && field.ends_with('"') {
        Some(&field[1..field.len() - 1])
    } else {
        None
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

        let fields = split_relay_array_fields(msg).ok_or(error::Error::DecodeFailed)?;
        let Some(command) = fields.first().and_then(|field| string_field(field)) else {
            return Err(error::Error::DecodeFailed);
        };

        match command {
            // Relay response format: ["NOTICE", <message>]
            "NOTICE" if fields.len() == 2 => string_field(fields[1])
                .map(Self::notice)
                .ok_or(error::Error::DecodeFailed),

            // Relay response format: ["EVENT", <subscription id>, <event JSON>]
            "EVENT" if fields.len() == 3 => {
                let subid = string_field(fields[1]).ok_or(error::Error::DecodeFailed)?;
                Ok(Self::event(fields[2], subid))
            }

            // Relay response format: ["EOSE", <subscription_id>]
            "EOSE" if fields.len() == 2 => string_field(fields[1])
                .map(Self::eose)
                .ok_or(error::Error::DecodeFailed),

            // Relay response format: ["OK", <event_id>, <true|false>, <message?>]
            "OK" if fields.len() == 3 || fields.len() == 4 => {
                let event_id = string_field(fields[1]).ok_or(error::Error::DecodeFailed)?;
                if event_id.len() != 64 {
                    return Err(error::Error::DecodeFailed);
                }
                let status = match fields[2] {
                    "true" => true,
                    "false" => false,
                    _ => return Err(error::Error::DecodeFailed),
                };
                let message = if fields.len() == 4 {
                    string_field(fields[3]).ok_or(error::Error::DecodeFailed)?
                } else {
                    ""
                };

                Ok(Self::ok(event_id, status, message))
            }

            // Relay response format: ["CLOSED", <subscription_id>, <message>]
            "CLOSED" if fields.len() == 3 => {
                let subid = string_field(fields[1]).ok_or(error::Error::DecodeFailed)?;
                let message = string_field(fields[2]).ok_or(error::Error::DecodeFailed)?;
                Ok(Self::Closed(subid, message))
            }

            // Relay request format: ["AUTH", <challenge-string>]
            "AUTH" if fields.len() == 2 => string_field(fields[1])
                .map(Self::Auth)
                .ok_or(error::Error::DecodeFailed),

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

    #[test]
    fn from_json_rejects_empty_input() {
        assert!(matches!(
            RelayMessage::from_json(""),
            Err(error::Error::Empty)
        ));
    }

    #[test]
    fn from_json_rejects_malformed_commands_without_panicking() {
        for malformed in [
            "[",
            "[]",
            r#"["EVENT"]"#,
            r#"["EVENT","sub"]"#,
            r#"["EOSE"]"#,
            r#"["NOTICE"]"#,
            r#"["CLOSED","sub"]"#,
            r#"["OK","short",true]"#,
            r#"["UNKNOWN","value"]"#,
        ] {
            assert!(
                matches!(
                    RelayMessage::from_json(malformed),
                    Err(error::Error::DecodeFailed)
                ),
                "malformed relay message should decode-fail: {malformed}"
            );
        }
    }

    #[test]
    fn from_json_rejects_truncated_or_missing_separator_arrays() {
        for malformed in [
            r#"["NOTICE","unterminated"#,
            r#"["EOSE","unterminated"#,
            r#"["AUTH","unterminated""#,
            r#"["AUTH""missing-comma"]"#,
            r#"["CLOSED","sub","unterminated"#,
        ] {
            assert!(
                matches!(
                    RelayMessage::from_json(malformed),
                    Err(error::Error::DecodeFailed)
                ),
                "malformed relay message should decode-fail instead of being parsed: {malformed}"
            );
        }
    }

    #[test]
    fn from_json_parses_ok_arrays_with_json_whitespace_and_rejects_unknown_status_tokens() {
        let event_id = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";

        assert_eq!(
            RelayMessage::from_json(&format!(
                r#"["OK", "{}", false, "blocked: policy"]"#,
                event_id
            ))
            .unwrap(),
            RelayMessage::ok(event_id, false, "blocked: policy")
        );
        assert!(matches!(
            RelayMessage::from_json(&format!(r#"["OK","{}",null,"bad status"]"#, event_id)),
            Err(error::Error::DecodeFailed)
        ));
    }

    #[test]
    fn from_json_accepts_notice_eose_and_closed_with_optional_command_spacing() {
        let cases = [
            (
                r#"["NOTICE","relay maintenance"]"#,
                RelayMessage::Notice("relay maintenance"),
            ),
            (
                r#"["NOTICE", "relay maintenance"]"#,
                RelayMessage::Notice("relay maintenance"),
            ),
            (r#"["EOSE","sub-1"]"#, RelayMessage::Eose("sub-1")),
            (r#"["EOSE", "sub-1"]"#, RelayMessage::Eose("sub-1")),
            (
                r#"["CLOSED","sub-1","auth-required: login first"]"#,
                RelayMessage::Closed("sub-1", "auth-required: login first"),
            ),
            (
                r#"["CLOSED", "sub-1", "auth-required: login first"]"#,
                RelayMessage::Closed("sub-1", "auth-required: login first"),
            ),
        ];

        for (raw, expected) in cases {
            assert_eq!(RelayMessage::from_json(raw).unwrap(), expected);
        }
    }

    #[test]
    fn from_json_preserves_event_payload_with_nested_json() {
        let raw = r#"["EVENT","mailbox",{"id":"abc","content":"keeps commas, brackets ] and nested JSON","tags":[["p","alice"],["e","event"]],"nested":{"ok":true,"values":[1,2,3]}}]"#;

        let parsed = RelayMessage::from_json(raw).unwrap();

        assert_eq!(
            parsed,
            RelayMessage::Event(
                "mailbox",
                r#"{"id":"abc","content":"keeps commas, brackets ] and nested JSON","tags":[["p","alice"],["e","event"]],"nested":{"ok":true,"values":[1,2,3]}}"#
            )
        );
    }

    #[test]
    fn from_json_accepts_event_messages_with_relay_spacing_variants() {
        let raw = r#"["EVENT",   "mailbox",   {"id":"abc","content":"payload"}]"#;

        let parsed = RelayMessage::from_json(raw).unwrap();

        assert_eq!(
            parsed,
            RelayMessage::Event("mailbox", r#"{"id":"abc","content":"payload"}"#)
        );
    }

    #[test]
    fn from_json_parses_ok_status_and_optional_message_variants() {
        let event_id = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        let cases = [
            (
                format!(r#"["OK","{}",true,""]"#, event_id),
                RelayMessage::ok(event_id, true, ""),
            ),
            (
                format!(r#"["OK","{}",false,"rate-limited: slow down"]"#, event_id),
                RelayMessage::ok(event_id, false, "rate-limited: slow down"),
            ),
            (
                format!(
                    r#"["OK","{}",false,"auth-required: please authenticate"]"#,
                    event_id
                ),
                RelayMessage::ok(event_id, false, "auth-required: please authenticate"),
            ),
            (
                format!(r#"["OK","{}",true]"#, event_id),
                RelayMessage::ok(event_id, true, ""),
            ),
        ];

        for (raw, expected) in cases {
            assert_eq!(RelayMessage::from_json(&raw).unwrap(), expected);
        }
    }

    #[test]
    fn client_message_from_subscription_serializes_filters_as_req_array_elements() {
        use crate::relay::Subscription;
        use nostr::Kind;
        use serde_json::json;

        let subscription = Subscription::new(
            "mailbox".to_string(),
            vec![
                Filter::new().kind(Kind::TextNote),
                Filter::new().kind(Kind::Metadata),
            ],
        );

        let serialized = serde_json::to_value(ClientMessage::from(subscription)).unwrap();

        assert_eq!(serialized[0], "REQ");
        assert_eq!(serialized[1], "mailbox");
        assert_eq!(serialized[2]["kinds"], json!([1]));
        assert_eq!(serialized[3]["kinds"], json!([0]));
        assert_eq!(serialized.as_array().unwrap().len(), 4);
    }

    #[test]
    fn client_message_serializes_req_event_and_auth_as_nip01_arrays() {
        use nostr::{EventBuilder, Keys, Kind, RelayUrl};
        use serde_json::json;

        let filter = Filter::new().kind(Kind::TextNote);
        let req = serde_json::to_value(&ClientMessage::Req {
            subscription_id: "mailbox".to_string(),
            filters: vec![filter],
        })
        .unwrap();
        assert_eq!(req[0], "REQ");
        assert_eq!(req[1], "mailbox");
        assert_eq!(req[2]["kinds"], json!([1]));

        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::TextNote, "hello relay")
            .sign_with_keys(&keys)
            .unwrap();
        let event_message = serde_json::to_value(&ClientMessage::Event {
            event: event.clone(),
        })
        .unwrap();
        assert_eq!(event_message[0], "EVENT");
        assert_eq!(event_message[1], serde_json::to_value(&event).unwrap());
        assert_eq!(event_message[1]["content"], "hello relay");

        let relay_url = RelayUrl::parse("wss://relay.example.com").unwrap();
        let auth_event = EventBuilder::auth("challenge", relay_url)
            .sign_with_keys(&keys)
            .unwrap();
        let auth_message = serde_json::to_value(&ClientMessage::Auth {
            event: auth_event.clone(),
        })
        .unwrap();
        assert_eq!(auth_message[0], "AUTH");
        assert_eq!(auth_message[1], serde_json::to_value(&auth_event).unwrap());
    }
}
