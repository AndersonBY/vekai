import argparse
import contextlib
import json
import os
import statistics
import threading
import time
from pathlib import Path

import torch
import torch.nn.functional as functional
from transformers import AutoModel, AutoTokenizer


def last_token_pool(last_hidden_state: torch.Tensor, attention_mask: torch.Tensor) -> torch.Tensor:
    left_padding = attention_mask[:, -1].sum() == attention_mask.shape[0]
    if left_padding:
        return last_hidden_state[:, -1]
    sequence_lengths = attention_mask.sum(dim=1) - 1
    batch_indices = torch.arange(last_hidden_state.shape[0], device=last_hidden_state.device)
    return last_hidden_state[batch_indices, sequence_lengths]


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[int((len(ordered) - 1) * fraction)]


def make_inputs(count: int) -> list[str]:
    return [
        (
            f"Document {index}: Vekai uses a local encoder for embedding and reranking. "
            "This input measures GPU batching, attention, and pooling performance."
        )
        for index in range(count)
    ]


def enable_windows_inductor_cache_workaround() -> None:
    if os.name != "nt":
        return

    import torch._inductor.codecache as codecache

    def write_atomic(
        path_value: str,
        content: str | bytes,
        make_dirs: bool = False,
        encode_utf_8: bool = False,
    ) -> None:
        path = Path(path_value)
        if make_dirs:
            path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.parent / f".{os.getpid()}.{threading.get_ident()}.tmp"
        mode = "w" if isinstance(content, str) else "wb"
        with temporary.open(mode, encoding="utf-8" if encode_utf_8 else None) as handle:
            handle.write(content)
        os.replace(temporary, path)

    codecache.write_atomic = write_atomic


class CudaGraphEncoder:
    def __init__(self, model: torch.nn.Module, batch_size: int, sequence_length: int) -> None:
        self.input_ids = torch.zeros(
            (batch_size, sequence_length), dtype=torch.long, device="cuda"
        )
        self.attention_mask = torch.ones(
            (batch_size, 1, sequence_length, sequence_length),
            dtype=torch.bool,
            device="cuda",
        )
        self.causal_mask = torch.ones(
            (sequence_length, sequence_length), dtype=torch.bool, device="cuda"
        ).tril_()

        warmup_stream = torch.cuda.Stream()
        warmup_stream.wait_stream(torch.cuda.current_stream())
        with torch.cuda.stream(warmup_stream):
            for _ in range(3):
                self._forward(model)
        torch.cuda.current_stream().wait_stream(warmup_stream)

        self.graph = torch.cuda.CUDAGraph()
        with torch.cuda.graph(self.graph):
            self.embeddings = self._forward(model)

    def _forward(self, model: torch.nn.Module) -> torch.Tensor:
        output = model(
            input_ids=self.input_ids,
            attention_mask=self.attention_mask,
            use_cache=False,
        )
        return functional.normalize(output.last_hidden_state[:, -1], p=2, dim=1)

    def encode(self, input_ids: torch.Tensor, attention_mask: torch.Tensor) -> torch.Tensor:
        self.input_ids.copy_(input_ids)
        expanded_mask = self.causal_mask[None, None] & attention_mask[:, None, None, :].bool()
        self.attention_mask.copy_(expanded_mask)
        self.graph.replay()
        return self.embeddings.clone()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", default="Qwen/Qwen3-Embedding-0.6B")
    parser.add_argument("--counts", default="1,8,32,64")
    parser.add_argument("--batch-sizes", default="1,8,16,32,64")
    parser.add_argument("--max-length", type=int, default=512)
    parser.add_argument("--warmup", type=int, default=3)
    parser.add_argument("--samples", type=int, default=10)
    parser.add_argument("--compile", action="store_true")
    parser.add_argument("--cuda-graph", action="store_true")
    parser.add_argument(
        "--natural-batch-only",
        action="store_true",
        help="Only run configurations where batch size equals input count.",
    )
    parser.add_argument(
        "--pad-to-length",
        type=int,
        default=0,
        help="Pad every batch to this fixed sequence length; required by --cuda-graph.",
    )
    parser.add_argument(
        "--cuda-profile",
        action="store_true",
        help="Wrap the first timed sample in cudaProfilerStart/Stop for Nsight capture.",
    )
    parser.add_argument("--dump-embeddings", default="")
    args = parser.parse_args()

    if args.cuda_graph and args.pad_to_length <= 0:
        parser.error("--cuda-graph requires --pad-to-length")

    if not torch.cuda.is_available():
        raise RuntimeError("CUDA is not available")

    torch.set_grad_enabled(False)
    torch.backends.cuda.matmul.allow_tf32 = True
    torch.backends.cudnn.allow_tf32 = True

    load_started = time.perf_counter()
    tokenizer = AutoTokenizer.from_pretrained(args.model, padding_side="left")
    model = AutoModel.from_pretrained(
        args.model,
        torch_dtype=torch.float16,
        attn_implementation="sdpa",
    ).eval().to("cuda")
    model.config.use_cache = False
    if args.compile:
        enable_windows_inductor_cache_workaround()
        model = torch.compile(model, mode="reduce-overhead", fullgraph=False)
    torch.cuda.synchronize()
    load_ms = (time.perf_counter() - load_started) * 1000

    results = []
    counts = [int(value) for value in args.counts.split(",")]
    batch_sizes = [int(value) for value in args.batch_sizes.split(",")]
    graph_encoders: dict[tuple[int, int], CudaGraphEncoder] = {}

    for count in counts:
        texts = make_inputs(count)
        for batch_size in batch_sizes:
            if batch_size > count:
                continue
            if args.natural_batch_only and batch_size != count:
                continue

            def encode() -> torch.Tensor:
                chunks = []
                for offset in range(0, len(texts), batch_size):
                    chunk = texts[offset : offset + batch_size]
                    padding: bool | str = "max_length" if args.pad_to_length else True
                    encoded = tokenizer(
                        chunk,
                        padding=padding,
                        truncation=True,
                        max_length=args.pad_to_length or args.max_length,
                        return_tensors="pt",
                    )
                    input_ids = encoded["input_ids"].to("cuda")
                    attention_mask = encoded["attention_mask"].to("cuda")
                    if args.cuda_graph:
                        key = (len(chunk), input_ids.shape[1])
                        graph_encoder = graph_encoders.get(key)
                        if graph_encoder is None:
                            graph_encoder = CudaGraphEncoder(model, *key)
                            graph_encoders[key] = graph_encoder
                        chunks.append(graph_encoder.encode(input_ids, attention_mask))
                    else:
                        output = model(
                            input_ids=input_ids,
                            attention_mask=attention_mask,
                            use_cache=False,
                        )
                        pooled = last_token_pool(output.last_hidden_state, attention_mask)
                        chunks.append(functional.normalize(pooled, p=2, dim=1))
                return torch.cat(chunks, dim=0)

            for _ in range(args.warmup):
                embeddings = encode()
                torch.cuda.synchronize()

            torch.cuda.reset_peak_memory_stats()
            samples = []
            for sample_index in range(args.samples):
                profile_this_sample = args.cuda_profile and sample_index == 0
                if profile_this_sample:
                    torch.cuda.cudart().cudaProfilerStart()
                started = time.perf_counter()
                with torch.cuda.nvtx.range("embedding.encode") if profile_this_sample else contextlib.nullcontext():
                    embeddings = encode()
                    torch.cuda.synchronize()
                samples.append((time.perf_counter() - started) * 1000)
                if profile_this_sample:
                    torch.cuda.cudart().cudaProfilerStop()

            results.append(
                {
                    "count": count,
                    "batch_size": batch_size,
                    "dimensions": embeddings.shape[1],
                    "p50_ms": round(percentile(samples, 0.5), 1),
                    "p90_ms": round(percentile(samples, 0.9), 1),
                    "mean_ms": round(statistics.mean(samples), 1),
                    "peak_vram_mib": round(torch.cuda.max_memory_allocated() / 1024 / 1024, 1),
                }
            )

    print(
        json.dumps(
            {
                "model": args.model,
                "device": torch.cuda.get_device_name(),
                "torch": torch.__version__,
                "compiled": args.compile,
                "cuda_graph": args.cuda_graph,
                "pad_to_length": args.pad_to_length,
                "load_ms": round(load_ms, 1),
                "results": results,
            },
            ensure_ascii=True,
            indent=2,
        )
    )
    if args.dump_embeddings:
        with open(args.dump_embeddings, "w", encoding="utf-8") as handle:
            json.dump(embeddings.float().cpu().tolist(), handle)


if __name__ == "__main__":
    main()
