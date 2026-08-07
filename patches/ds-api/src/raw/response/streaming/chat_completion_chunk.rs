use serde::Deserialize;

use super::{chunk_choice::ChunkChoice, chunk_object_type::ChunkObjectType};
use crate::raw::response::non_streaming::Usage;

#[derive(Debug, Deserialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub choices: Vec<ChunkChoice>,
    pub created: u64,
    pub model: String,
    pub system_fingerprint: String,
    #[serde(rename = "object")]
    pub object: ChunkObjectType,
    #[serde(default)]
    pub usage: Option<Usage>,
}
