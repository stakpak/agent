use crate::types::{ChannelId, ChatType, PeerId};

#[derive(Debug, Clone, Default)]
pub struct RouterConfig {
    pub dm_scope: DmScope,
    pub bindings: Vec<Binding>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub enum DmScope {
    Main,
    PerPeer,
    #[default]
    PerChannelPeer,
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub match_rule: BindingMatch,
    pub routing_key: String,
}

#[derive(Debug, Clone)]
pub struct BindingMatch {
    pub channel: ChannelId,
    pub peer: Option<PeerMatch>,
}

#[derive(Debug, Clone)]
pub struct PeerMatch {
    pub kind: PeerMatchKind,
    pub id: String,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PeerMatchKind {
    Direct,
    Group,
}

pub fn resolve_routing_key(
    config: &RouterConfig,
    channel: &ChannelId,
    peer_id: &PeerId,
    chat_type: &ChatType,
) -> String {
    if let Some(binding) = config
        .bindings
        .iter()
        .find(|binding| binding_matches_peer(binding, channel, peer_id, chat_type))
    {
        return binding.routing_key.clone();
    }

    if let Some(binding) = config
        .bindings
        .iter()
        .find(|binding| binding_matches_channel(binding, channel))
    {
        return binding.routing_key.clone();
    }

    match chat_type {
        ChatType::Direct => match config.dm_scope {
            DmScope::Main => "main".to_string(),
            DmScope::PerPeer => format!("dm:{}", peer_id.0),
            DmScope::PerChannelPeer => format!("{}:dm:{}", channel.0, peer_id.0),
        },
        ChatType::Group { id } => format!("{}:group:{id}", channel.0),
        ChatType::Thread {
            group_id,
            thread_id,
        } => {
            format!("{}:thread:{group_id}:{thread_id}", channel.0)
        }
    }
}

fn binding_matches_peer(
    binding: &Binding,
    channel: &ChannelId,
    peer_id: &PeerId,
    chat_type: &ChatType,
) -> bool {
    if binding.match_rule.channel != *channel {
        return false;
    }

    let Some(peer_match) = binding.match_rule.peer.as_ref() else {
        return false;
    };

    match peer_match.kind {
        PeerMatchKind::Direct => {
            matches!(chat_type, ChatType::Direct) && peer_match.id == peer_id.0
        }
        PeerMatchKind::Group => match chat_type {
            ChatType::Group { id } => peer_match.id == *id,
            ChatType::Thread { group_id, .. } => peer_match.id == *group_id,
            ChatType::Direct => false,
        },
    }
}

fn binding_matches_channel(binding: &Binding, channel: &ChannelId) -> bool {
    binding.match_rule.channel == *channel && binding.match_rule.peer.is_none()
}

#[cfg(test)]
mod tests {
    use super::{
        Binding, BindingMatch, DmScope, PeerMatch, PeerMatchKind, RouterConfig, resolve_routing_key,
    };
    use crate::types::{ChannelId, ChatType, PeerId};

    #[test]
    fn default_dm_scope_is_per_channel_peer() {
        let config = RouterConfig::default();
        assert_eq!(config.dm_scope, DmScope::PerChannelPeer);
    }

    #[test]
    fn resolves_direct_main_scope() {
        let config = RouterConfig {
            dm_scope: DmScope::Main,
            bindings: Vec::new(),
        };

        let key = resolve_routing_key(
            &config,
            &ChannelId::from("telegram"),
            &PeerId::from("123"),
            &ChatType::Direct,
        );

        assert_eq!(key, "main");
    }

    #[test]
    fn resolves_direct_per_peer_scope() {
        let config = RouterConfig {
            dm_scope: DmScope::PerPeer,
            bindings: Vec::new(),
        };

        let key = resolve_routing_key(
            &config,
            &ChannelId::from("telegram"),
            &PeerId::from("123"),
            &ChatType::Direct,
        );

        assert_eq!(key, "dm:123");
    }

    #[test]
    fn resolves_direct_per_channel_peer_scope() {
        let config = RouterConfig::default();

        let key = resolve_routing_key(
            &config,
            &ChannelId::from("telegram"),
            &PeerId::from("123"),
            &ChatType::Direct,
        );

        assert_eq!(key, "telegram:dm:123");
    }

    #[test]
    fn resolves_group_key() {
        let config = RouterConfig::default();

        let key = resolve_routing_key(
            &config,
            &ChannelId::from("telegram"),
            &PeerId::from("123"),
            &ChatType::Group {
                id: "-100999".to_string(),
            },
        );

        assert_eq!(key, "telegram:group:-100999");
    }

    #[test]
    fn resolves_thread_key() {
        let config = RouterConfig::default();

        let key = resolve_routing_key(
            &config,
            &ChannelId::from("telegram"),
            &PeerId::from("123"),
            &ChatType::Thread {
                group_id: "-100999".to_string(),
                thread_id: "42".to_string(),
            },
        );

        assert_eq!(key, "telegram:thread:-100999:42");
    }

    #[test]
    fn peer_binding_overrides_channel_binding_and_default() {
        let config = RouterConfig {
            dm_scope: DmScope::PerChannelPeer,
            bindings: vec![
                Binding {
                    match_rule: BindingMatch {
                        channel: ChannelId::from("telegram"),
                        peer: Some(PeerMatch {
                            kind: PeerMatchKind::Direct,
                            id: "123".to_string(),
                        }),
                    },
                    routing_key: "peer-bound".to_string(),
                },
                Binding {
                    match_rule: BindingMatch {
                        channel: ChannelId::from("telegram"),
                        peer: None,
                    },
                    routing_key: "channel-bound".to_string(),
                },
            ],
        };

        let key = resolve_routing_key(
            &config,
            &ChannelId::from("telegram"),
            &PeerId::from("123"),
            &ChatType::Direct,
        );

        assert_eq!(key, "peer-bound");
    }

    #[test]
    fn channel_binding_overrides_default_when_no_peer_match() {
        let config = RouterConfig {
            dm_scope: DmScope::PerChannelPeer,
            bindings: vec![Binding {
                match_rule: BindingMatch {
                    channel: ChannelId::from("telegram"),
                    peer: None,
                },
                routing_key: "channel-bound".to_string(),
            }],
        };

        let key = resolve_routing_key(
            &config,
            &ChannelId::from("telegram"),
            &PeerId::from("123"),
            &ChatType::Direct,
        );

        assert_eq!(key, "channel-bound");
    }

    #[test]
    fn group_peer_binding_matches_group_and_thread_parent() {
        let config = RouterConfig {
            dm_scope: DmScope::PerChannelPeer,
            bindings: vec![Binding {
                match_rule: BindingMatch {
                    channel: ChannelId::from("telegram"),
                    peer: Some(PeerMatch {
                        kind: PeerMatchKind::Group,
                        id: "-100999".to_string(),
                    }),
                },
                routing_key: "group-bound".to_string(),
            }],
        };

        let group_key = resolve_routing_key(
            &config,
            &ChannelId::from("telegram"),
            &PeerId::from("123"),
            &ChatType::Group {
                id: "-100999".to_string(),
            },
        );

        let thread_key = resolve_routing_key(
            &config,
            &ChannelId::from("telegram"),
            &PeerId::from("123"),
            &ChatType::Thread {
                group_id: "-100999".to_string(),
                thread_id: "44".to_string(),
            },
        );

        assert_eq!(group_key, "group-bound");
        assert_eq!(thread_key, "group-bound");
    }
}
