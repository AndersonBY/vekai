import argparse
import json
import statistics
import time
import urllib.request


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[int((len(ordered) - 1) * fraction)]


def make_texts(count: int) -> list[str]:
    return [
        (
            f"Document {index}: Vekai uses a local encoder for embedding and reranking. "
            "This input measures GPU batching, attention, and pooling performance."
        )
        for index in range(count)
    ]


def post_json(url: str, payload: dict) -> tuple[dict, int]:
    body = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=120) as response:
        response_body = response.read()
    return json.loads(response_body), len(response_body)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", required=True)
    parser.add_argument("--service", choices=("embedding", "rerank"), required=True)
    parser.add_argument("--counts", default="1,8,32,64")
    parser.add_argument("--warmup", type=int, default=3)
    parser.add_argument("--samples", type=int, default=10)
    parser.add_argument("--dump-response", default="")
    args = parser.parse_args()

    results = []
    last_response = None
    for count in (int(value) for value in args.counts.split(",")):
        texts = make_texts(count)
        payload = (
            {"model": "local-embedding", "input": texts, "encoding_format": "float"}
            if args.service == "embedding"
            else {
                "model": "local-reranker",
                "query": "Which document describes Vekai retrieval performance?",
                "documents": texts,
                "top_n": count,
            }
        )

        for _ in range(args.warmup):
            post_json(args.url, payload)

        samples = []
        response_bytes = 0
        for _ in range(args.samples):
            started = time.perf_counter()
            last_response, response_bytes = post_json(args.url, payload)
            samples.append((time.perf_counter() - started) * 1000)

        result = {
            "count": count,
            "p50_ms": round(percentile(samples, 0.5), 1),
            "p90_ms": round(percentile(samples, 0.9), 1),
            "mean_ms": round(statistics.mean(samples), 1),
            "response_bytes": response_bytes,
        }
        if args.service == "embedding" and last_response:
            result["dimensions"] = len(last_response["data"][0]["embedding"])
        results.append(result)

    if args.dump_response and last_response is not None:
        with open(args.dump_response, "w", encoding="utf-8") as handle:
            json.dump(last_response, handle)

    print(json.dumps({"service": args.service, "results": results}, indent=2))


if __name__ == "__main__":
    main()
