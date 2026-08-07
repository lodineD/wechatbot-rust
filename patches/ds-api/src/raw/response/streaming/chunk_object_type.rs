use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub enum ChunkObjectType {
    #[serde(rename = "chat.completion.chunk")]
    ChatCompletionChunk,
}
