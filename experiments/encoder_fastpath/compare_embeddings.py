import argparse
import json
import statistics


def cosine(left: list[float], right: list[float]) -> float:
    dot = sum(a * b for a, b in zip(left, right, strict=True))
    left_norm = sum(value * value for value in left) ** 0.5
    right_norm = sum(value * value for value in right) ** 0.5
    return dot / (left_norm * right_norm)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fp16", required=True)
    parser.add_argument("--llama", required=True)
    args = parser.parse_args()

    with open(args.fp16, encoding="utf-8") as handle:
        fp16_embeddings = json.load(handle)
    with open(args.llama, encoding="utf-8") as handle:
        llama_response = json.load(handle)
    llama_embeddings = [
        item["embedding"] for item in sorted(llama_response["data"], key=lambda item: item["index"])
    ]

    if len(fp16_embeddings) != len(llama_embeddings):
        raise ValueError(
            f"embedding count mismatch: FP16={len(fp16_embeddings)}, llama={len(llama_embeddings)}"
        )
    similarities = [
        cosine(fp16, llama)
        for fp16, llama in zip(fp16_embeddings, llama_embeddings, strict=True)
    ]
    print(
        json.dumps(
            {
                "count": len(similarities),
                "min_cosine": min(similarities),
                "mean_cosine": statistics.mean(similarities),
                "max_cosine": max(similarities),
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
