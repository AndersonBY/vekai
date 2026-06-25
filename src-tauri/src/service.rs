use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServiceKind {
    Rerank,
    Embedding,
}

impl ServiceKind {
    pub const ALL: [Self; 2] = [Self::Rerank, Self::Embedding];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Rerank => "Rerank",
            Self::Embedding => "Embedding",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Rerank => "rerank",
            Self::Embedding => "embedding",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Self::Rerank => 8012,
            Self::Embedding => 8013,
        }
    }
}
