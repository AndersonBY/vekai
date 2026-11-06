use serde_json::{json, Map, Value};

pub fn embedding_request_for_llama(protocol: &str, incoming: Value) -> Value {
    let model = incoming
        .get("model")
        .cloned()
        .unwrap_or_else(|| Value::String("local-embedding".to_string()));
    let input = if protocol == "cohere_embed_v2" {
        incoming
            .get("texts")
            .cloned()
            .or_else(|| incoming.get("input").cloned())
            .unwrap_or_else(|| Value::Array(Vec::new()))
    } else {
        incoming
            .get("input")
            .cloned()
            .or_else(|| incoming.get("texts").cloned())
            .unwrap_or_else(|| Value::String(String::new()))
    };

    let mut request = Map::new();
    request.insert("model".to_string(), model);
    request.insert("input".to_string(), input);
    request.insert(
        "encoding_format".to_string(),
        Value::String("float".to_string()),
    );

    if let Some(dimensions) = incoming
        .get("dimensions")
        .or_else(|| incoming.get("output_dimension"))
        .cloned()
    {
        request.insert("dimensions".to_string(), dimensions);
    }

    Value::Object(request)
}

pub fn embedding_response_for_protocol(protocol: &str, raw: Value) -> Value {
    if protocol != "cohere_embed_v2" {
        return raw;
    }

    let vectors = raw
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("embedding").cloned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let input_tokens = raw
        .pointer("/usage/prompt_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);

    json!({
        "embeddings": {
            "float": vectors
        },
        "meta": {
            "billed_units": {
                "input_tokens": input_tokens
            }
        }
    })
}

pub fn rerank_request_for_llama(_protocol: &str, incoming: Value) -> (Value, Vec<Value>) {
    let model = incoming
        .get("model")
        .cloned()
        .unwrap_or_else(|| Value::String("local-reranker".to_string()));
    let query = incoming
        .get("query")
        .or_else(|| incoming.get("q"))
        .cloned()
        .unwrap_or_else(|| Value::String(String::new()));
    let documents = incoming
        .get("documents")
        .or_else(|| incoming.get("docs"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(normalize_document_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut request = Map::new();
    request.insert("model".to_string(), model);
    request.insert("query".to_string(), query);
    request.insert("documents".to_string(), Value::Array(documents.clone()));

    if let Some(top_n) = incoming
        .get("top_n")
        .or_else(|| incoming.get("top_k"))
        .or_else(|| incoming.get("limit"))
        .cloned()
    {
        if !top_n.is_null() {
            request.insert("top_n".to_string(), top_n);
        }
    }

    (Value::Object(request), documents)
}

pub fn rerank_response_for_protocol(protocol: &str, raw: Value, documents: &[Value]) -> Value {
    if protocol == "llama_rerank" {
        return raw;
    }

    let model = raw
        .get("model")
        .cloned()
        .unwrap_or_else(|| Value::String("local-reranker".to_string()));
    let total_tokens = raw
        .pointer("/usage/total_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let normalized = normalized_rerank_results(&raw, documents);

    match protocol {
        "cohere_rerank_v2" => json!({
            "results": normalized
                .iter()
                .map(|item| {
                    json!({
                        "index": item.index,
                        "relevance_score": item.score,
                        "document": {"text": item.document_text.clone()}
                    })
                })
                .collect::<Vec<_>>(),
            "meta": {
                "billed_units": {"search_units": 1}
            }
        }),
        "siliconflow" => json!({
            "model": model,
            "results": normalized
                .iter()
                .map(|item| {
                    json!({
                        "index": item.index,
                        "relevance_score": item.score,
                        "document": {"text": item.document_text.clone()}
                    })
                })
                .collect::<Vec<_>>(),
            "meta": {
                "billed_units": {"search_units": 1},
                "tokens": {"input_tokens": total_tokens}
            }
        }),
        "voyage_rerank_v1" => json!({
            "model": model,
            "data": normalized
                .iter()
                .map(|item| {
                    json!({
                        "index": item.index,
                        "relevance_score": item.score,
                        "document": item.document.clone()
                    })
                })
                .collect::<Vec<_>>(),
            "usage": {"total_tokens": total_tokens}
        }),
        _ => json!({
            "model": model,
            "results": normalized
                .iter()
                .map(|item| {
                    json!({
                        "index": item.index,
                        "relevance_score": item.score,
                        "document": item.document.clone()
                    })
                })
                .collect::<Vec<_>>(),
            "usage": {"total_tokens": total_tokens}
        }),
    }
}

#[derive(Debug)]
struct NormalizedRerankItem {
    index: usize,
    score: f64,
    document: Value,
    document_text: String,
}

fn normalized_rerank_results(raw: &Value, documents: &[Value]) -> Vec<NormalizedRerankItem> {
    raw.get("results")
        .or_else(|| raw.get("data"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .enumerate()
                .map(|(fallback_index, item)| {
                    normalized_rerank_item(item, fallback_index, documents)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn normalized_rerank_item(
    item: &Value,
    fallback_index: usize,
    documents: &[Value],
) -> NormalizedRerankItem {
    let index = item
        .get("index")
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(fallback_index);
    let score = item
        .get("relevance_score")
        .or_else(|| item.get("score"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let document = item
        .get("document")
        .cloned()
        .or_else(|| documents.get(index).cloned())
        .unwrap_or(Value::Null);
    let document = if document.get("image").is_some() {
        json!({"text": document.get("text").cloned().unwrap_or(json!(""))})
    } else {
        document
    };
    let document_text = document_text(&document);

    NormalizedRerankItem {
        index,
        score,
        document,
        document_text,
    }
}

fn normalize_document_value(value: &Value) -> Value {
    if value.get("image").is_some() || value.get("text").is_some_and(Value::is_array) {
        return value.clone();
    }
    if let Some(text) = value
        .as_object()
        .and_then(|object| object.get("text"))
        .and_then(Value::as_str)
    {
        return Value::String(text.to_string());
    }

    if let Some(text) = value.as_str() {
        return Value::String(text.to_string());
    }

    Value::String(value.to_string())
}

fn document_text(value: &Value) -> String {
    if let Some(texts) = value.get("text").and_then(Value::as_array) {
        return texts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n");
    }
    value
        .as_object()
        .and_then(|object| object.get("text"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| value.as_str().map(str::to_string))
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{embedding_request_for_llama, embedding_response_for_protocol};
    use super::{rerank_request_for_llama, rerank_response_for_protocol};

    #[test]
    fn multimodal_rerank_preserves_inputs_and_omits_response_images() {
        let image = json!({"image": "data:image/png;base64,secret"});
        let mixed =
            json!({"text": ["caption", "detail"], "image": ["data:image/png;base64,secret"]});
        let (request, docs) = rerank_request_for_llama(
            "jina_rerank_v1",
            json!({"query":image,"documents":[image,mixed]}),
        );
        assert_eq!(request["query"], image);
        assert_eq!(request["documents"], json!([image, mixed]));
        for protocol in [
            "jina_rerank_v1",
            "cohere_rerank_v2",
            "siliconflow",
            "voyage_rerank_v1",
            "custom_json_http",
        ] {
            let response = rerank_response_for_protocol(
                protocol,
                json!({"results":[{"index":0,"relevance_score":0.9},{"index":1,"relevance_score":0.8,"document":mixed}]}),
                &docs,
            );
            assert!(!response.to_string().contains("base64"), "{protocol}");
            assert!(response.to_string().contains("caption"));
            if protocol == "cohere_rerank_v2" {
                assert_eq!(
                    response["results"][1]["document"]["text"],
                    "caption\ndetail"
                );
            }
        }
    }

    #[test]
    fn cohere_embedding_protocol_maps_texts_to_openai_request_and_back() {
        let request = embedding_request_for_llama(
            "cohere_embed_v2",
            json!({
                "model": "embed-v4.0",
                "texts": ["alpha", "beta"],
                "input_type": "search_document"
            }),
        );

        assert_eq!(
            request,
            json!({
                "model": "embed-v4.0",
                "input": ["alpha", "beta"],
                "encoding_format": "float"
            })
        );

        let response = embedding_response_for_protocol(
            "cohere_embed_v2",
            json!({
                "model": "embed-v4.0",
                "data": [
                    {"index": 0, "embedding": [0.1, 0.2]},
                    {"index": 1, "embedding": [0.3, 0.4]}
                ],
                "usage": {"prompt_tokens": 4, "total_tokens": 4}
            }),
        );

        assert_eq!(
            response,
            json!({
                "embeddings": {
                    "float": [[0.1, 0.2], [0.3, 0.4]]
                },
                "meta": {
                    "billed_units": {
                        "input_tokens": 4
                    }
                }
            })
        );
    }

    #[test]
    fn siliconflow_rerank_protocol_adds_document_text_and_usage_meta() {
        let (request, documents) = rerank_request_for_llama(
            "siliconflow",
            json!({
                "model": "BAAI/bge-reranker-v2-m3",
                "query": "apple",
                "documents": ["banana", "apple"]
            }),
        );

        assert_eq!(
            request,
            json!({
                "model": "BAAI/bge-reranker-v2-m3",
                "query": "apple",
                "documents": ["banana", "apple"]
            })
        );

        let response = rerank_response_for_protocol(
            "siliconflow",
            json!({
                "model": "BAAI/bge-reranker-v2-m3",
                "results": [
                    {"index": 1, "relevance_score": 0.97},
                    {"index": 0, "score": 0.21}
                ],
                "usage": {"total_tokens": 18}
            }),
            &documents,
        );

        assert_eq!(
            response,
            json!({
                "model": "BAAI/bge-reranker-v2-m3",
                "results": [
                    {"index": 1, "relevance_score": 0.97, "document": {"text": "apple"}},
                    {"index": 0, "relevance_score": 0.21, "document": {"text": "banana"}}
                ],
                "meta": {
                    "billed_units": {"search_units": 1},
                    "tokens": {"input_tokens": 18}
                }
            })
        );
    }

    #[test]
    fn cohere_rerank_protocol_uses_top_n_and_document_text_objects() {
        let (request, documents) = rerank_request_for_llama(
            "cohere_rerank_v2",
            json!({
                "model": "rerank-v3.5",
                "query": "q",
                "documents": [{"text": "doc1"}, {"text": "doc2"}],
                "top_n": 1,
                "return_documents": true
            }),
        );

        assert_eq!(
            request,
            json!({
                "model": "rerank-v3.5",
                "query": "q",
                "documents": ["doc1", "doc2"],
                "top_n": 1
            })
        );

        let response = rerank_response_for_protocol(
            "cohere_rerank_v2",
            json!({
                "results": [
                    {"index": 1, "relevance_score": 0.92}
                ]
            }),
            &documents,
        );

        assert_eq!(
            response,
            json!({
                "results": [
                    {"index": 1, "relevance_score": 0.92, "document": {"text": "doc2"}}
                ],
                "meta": {
                    "billed_units": {"search_units": 1}
                }
            })
        );
    }
}
