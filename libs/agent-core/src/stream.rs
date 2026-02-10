use serde_json::Value;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum IndexedStreamEvent {
    TextDelta {
        content_index: usize,
        delta: String,
    },
    ThinkingDelta {
        content_index: usize,
        delta: String,
    },
    ToolCallStart {
        content_index: usize,
        id: String,
        name: String,
    },
    ToolCallArgumentsDelta {
        content_index: usize,
        id: String,
        delta: String,
    },
    ToolCallEnd {
        content_index: usize,
        id: String,
        name: String,
        arguments: Value,
        metadata: Option<Value>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderedContentPart {
    Text(String),
    Thinking(String),
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
        metadata: Option<Value>,
    },
}

#[derive(Debug, Error)]
pub enum StreamAssemblyError {
    #[error("content index {content_index} changed slot type during streaming")]
    ContentTypeMismatch { content_index: usize },

    #[error("tool call id mismatch at content index {content_index}")]
    ToolCallIdMismatch { content_index: usize },

    #[error("invalid tool call arguments for {tool_call_id}: {source}")]
    InvalidToolCallArguments {
        tool_call_id: String,
        #[source]
        source: serde_json::Error,
    },
}

impl PartialEq for StreamAssemblyError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                StreamAssemblyError::ContentTypeMismatch {
                    content_index: left,
                },
                StreamAssemblyError::ContentTypeMismatch {
                    content_index: right,
                },
            ) => left == right,
            (
                StreamAssemblyError::ToolCallIdMismatch {
                    content_index: left,
                },
                StreamAssemblyError::ToolCallIdMismatch {
                    content_index: right,
                },
            ) => left == right,
            (
                StreamAssemblyError::InvalidToolCallArguments {
                    tool_call_id: left, ..
                },
                StreamAssemblyError::InvalidToolCallArguments {
                    tool_call_id: right,
                    ..
                },
            ) => left == right,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ContentSlot {
    Text(String),
    Thinking(String),
    ToolCall(ToolCallSlot),
}

#[derive(Debug, Clone, PartialEq)]
struct ToolCallSlot {
    id: String,
    name: String,
    arguments_buffer: String,
    final_arguments: Option<Value>,
    metadata: Option<Value>,
}

impl ToolCallSlot {
    fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            arguments_buffer: String::new(),
            final_arguments: None,
            metadata: None,
        }
    }

    fn into_part(self) -> Result<OrderedContentPart, StreamAssemblyError> {
        let arguments = if let Some(arguments) = self.final_arguments {
            arguments
        } else if self.arguments_buffer.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(&self.arguments_buffer).map_err(|source| {
                StreamAssemblyError::InvalidToolCallArguments {
                    tool_call_id: self.id.clone(),
                    source,
                }
            })?
        };

        Ok(OrderedContentPart::ToolCall {
            id: self.id,
            name: self.name,
            arguments,
            metadata: self.metadata,
        })
    }
}

pub fn assemble_ordered_content(
    events: impl IntoIterator<Item = IndexedStreamEvent>,
) -> Result<Vec<OrderedContentPart>, StreamAssemblyError> {
    let mut slots: BTreeMap<usize, ContentSlot> = BTreeMap::new();

    for event in events {
        match event {
            IndexedStreamEvent::TextDelta {
                content_index,
                delta,
            } => match slots.get_mut(&content_index) {
                Some(ContentSlot::Text(text)) => text.push_str(&delta),
                Some(_) => {
                    return Err(StreamAssemblyError::ContentTypeMismatch { content_index });
                }
                None => {
                    slots.insert(content_index, ContentSlot::Text(delta));
                }
            },
            IndexedStreamEvent::ThinkingDelta {
                content_index,
                delta,
            } => match slots.get_mut(&content_index) {
                Some(ContentSlot::Thinking(text)) => text.push_str(&delta),
                Some(_) => {
                    return Err(StreamAssemblyError::ContentTypeMismatch { content_index });
                }
                None => {
                    slots.insert(content_index, ContentSlot::Thinking(delta));
                }
            },
            IndexedStreamEvent::ToolCallStart {
                content_index,
                id,
                name,
            } => match slots.get_mut(&content_index) {
                Some(ContentSlot::ToolCall(slot)) => {
                    if slot.id != id {
                        return Err(StreamAssemblyError::ToolCallIdMismatch { content_index });
                    }
                    if slot.name.is_empty() {
                        slot.name = name;
                    }
                }
                Some(_) => {
                    return Err(StreamAssemblyError::ContentTypeMismatch { content_index });
                }
                None => {
                    slots.insert(
                        content_index,
                        ContentSlot::ToolCall(ToolCallSlot::new(id, name)),
                    );
                }
            },
            IndexedStreamEvent::ToolCallArgumentsDelta {
                content_index,
                id,
                delta,
            } => match slots.get_mut(&content_index) {
                Some(ContentSlot::ToolCall(slot)) => {
                    if slot.id != id {
                        return Err(StreamAssemblyError::ToolCallIdMismatch { content_index });
                    }
                    slot.arguments_buffer.push_str(&delta);
                }
                Some(_) => {
                    return Err(StreamAssemblyError::ContentTypeMismatch { content_index });
                }
                None => {
                    let mut slot = ToolCallSlot::new(id, String::new());
                    slot.arguments_buffer.push_str(&delta);
                    slots.insert(content_index, ContentSlot::ToolCall(slot));
                }
            },
            IndexedStreamEvent::ToolCallEnd {
                content_index,
                id,
                name,
                arguments,
                metadata,
            } => match slots.get_mut(&content_index) {
                Some(ContentSlot::ToolCall(slot)) => {
                    if slot.id != id {
                        return Err(StreamAssemblyError::ToolCallIdMismatch { content_index });
                    }
                    if slot.name.is_empty() {
                        slot.name = name;
                    }
                    slot.final_arguments = Some(arguments);
                    slot.metadata = metadata;
                }
                Some(_) => {
                    return Err(StreamAssemblyError::ContentTypeMismatch { content_index });
                }
                None => {
                    let mut slot = ToolCallSlot::new(id, name);
                    slot.final_arguments = Some(arguments);
                    slot.metadata = metadata;
                    slots.insert(content_index, ContentSlot::ToolCall(slot));
                }
            },
        }
    }

    slots
        .into_values()
        .map(|slot| match slot {
            ContentSlot::Text(text) => Ok(OrderedContentPart::Text(text)),
            ContentSlot::Thinking(text) => Ok(OrderedContentPart::Thinking(text)),
            ContentSlot::ToolCall(slot) => slot.into_part(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_index_order_for_out_of_order_events() {
        let parts = assemble_ordered_content(vec![
            IndexedStreamEvent::TextDelta {
                content_index: 2,
                delta: "third".to_string(),
            },
            IndexedStreamEvent::TextDelta {
                content_index: 0,
                delta: "first".to_string(),
            },
            IndexedStreamEvent::TextDelta {
                content_index: 1,
                delta: "second".to_string(),
            },
        ]);

        assert_eq!(
            parts,
            Ok(vec![
                OrderedContentPart::Text("first".to_string()),
                OrderedContentPart::Text("second".to_string()),
                OrderedContentPart::Text("third".to_string()),
            ])
        );
    }

    #[test]
    fn preserves_text_tool_call_thinking_interleaving() {
        let parts = assemble_ordered_content(vec![
            IndexedStreamEvent::TextDelta {
                content_index: 0,
                delta: "check logs".to_string(),
            },
            IndexedStreamEvent::ToolCallStart {
                content_index: 1,
                id: "tc_1".to_string(),
                name: "stakpak__run_command".to_string(),
            },
            IndexedStreamEvent::ToolCallArgumentsDelta {
                content_index: 1,
                id: "tc_1".to_string(),
                delta: "{\"cmd\":\"kubectl get pods\"}".to_string(),
            },
            IndexedStreamEvent::ThinkingDelta {
                content_index: 2,
                delta: "observing cluster state".to_string(),
            },
        ]);

        assert_eq!(
            parts,
            Ok(vec![
                OrderedContentPart::Text("check logs".to_string()),
                OrderedContentPart::ToolCall {
                    id: "tc_1".to_string(),
                    name: "stakpak__run_command".to_string(),
                    arguments: json!({"cmd":"kubectl get pods"}),
                    metadata: None,
                },
                OrderedContentPart::Thinking("observing cluster state".to_string()),
            ])
        );
    }

    #[test]
    fn accepts_tool_call_end_without_start() {
        let parts = assemble_ordered_content(vec![IndexedStreamEvent::ToolCallEnd {
            content_index: 0,
            id: "tc_1".to_string(),
            name: "stakpak__view".to_string(),
            arguments: json!({"path":"README.md"}),
            metadata: Some(json!({"provider":"gemini"})),
        }]);

        assert_eq!(
            parts,
            Ok(vec![OrderedContentPart::ToolCall {
                id: "tc_1".to_string(),
                name: "stakpak__view".to_string(),
                arguments: json!({"path":"README.md"}),
                metadata: Some(json!({"provider":"gemini"})),
            }])
        );
    }

    #[test]
    fn errors_on_content_type_mismatch_for_same_index() {
        let result = assemble_ordered_content(vec![
            IndexedStreamEvent::TextDelta {
                content_index: 0,
                delta: "hello".to_string(),
            },
            IndexedStreamEvent::ToolCallStart {
                content_index: 0,
                id: "tc_1".to_string(),
                name: "stakpak__view".to_string(),
            },
        ]);

        assert_eq!(
            result,
            Err(StreamAssemblyError::ContentTypeMismatch { content_index: 0 })
        );
    }

    #[test]
    fn errors_on_invalid_buffered_tool_arguments() {
        let result = assemble_ordered_content(vec![
            IndexedStreamEvent::ToolCallStart {
                content_index: 0,
                id: "tc_1".to_string(),
                name: "stakpak__view".to_string(),
            },
            IndexedStreamEvent::ToolCallArgumentsDelta {
                content_index: 0,
                id: "tc_1".to_string(),
                delta: "{not json".to_string(),
            },
        ]);

        assert!(matches!(
            result,
            Err(StreamAssemblyError::InvalidToolCallArguments { tool_call_id, .. })
                if tool_call_id == "tc_1"
        ));
    }
}
