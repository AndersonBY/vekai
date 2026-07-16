import argparse
import json
import time
from pathlib import Path

import onnx
import torch
import torch.nn.functional as functional
from transformers import AutoModel, AutoTokenizer


class EmbeddingModel(torch.nn.Module):
    def __init__(self, model: torch.nn.Module) -> None:
        super().__init__()
        self.model = model

    def forward(
        self, input_ids: torch.Tensor, attention_mask: torch.Tensor
    ) -> torch.Tensor:
        sequence_length = input_ids.shape[1]
        causal_mask = torch.ones(
            (sequence_length, sequence_length), dtype=torch.bool, device=input_ids.device
        ).tril_()
        expanded_mask = causal_mask[None, None] & attention_mask[:, None, None, :].bool()
        padding_query_rows = ~attention_mask[:, None, :, None].bool()
        expanded_mask = expanded_mask | padding_query_rows
        position_ids = torch.arange(sequence_length, device=input_ids.device)[None, :]
        output = self.model(
            input_ids=input_ids,
            attention_mask=expanded_mask,
            position_ids=position_ids,
            use_cache=False,
        )
        return functional.normalize(output.last_hidden_state[:, -1], p=2, dim=1)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", default="Qwen/Qwen3-Embedding-0.6B")
    parser.add_argument("--output", required=True)
    parser.add_argument("--sequence-length", type=int, default=32)
    args = parser.parse_args()

    output_path = Path(args.output).resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)

    tokenizer = AutoTokenizer.from_pretrained(args.model, padding_side="left")
    encoded = tokenizer(
        ["Vekai ONNX export example."],
        padding="max_length",
        truncation=True,
        max_length=args.sequence_length,
        return_tensors="pt",
    ).to("cuda")

    load_started = time.perf_counter()
    model = AutoModel.from_pretrained(
        args.model,
        torch_dtype=torch.float16,
        attn_implementation="sdpa",
    ).eval().to("cuda")
    model.config.use_cache = False
    wrapper = EmbeddingModel(model).eval()
    torch.cuda.synchronize()
    load_ms = (time.perf_counter() - load_started) * 1000

    export_started = time.perf_counter()
    with torch.inference_mode():
        torch.onnx.export(
            wrapper,
            (encoded["input_ids"], encoded["attention_mask"]),
            output_path,
            input_names=["input_ids", "attention_mask"],
            output_names=["embeddings"],
            opset_version=18,
            dynamic_axes={
                "input_ids": {0: "batch", 1: "sequence"},
                "attention_mask": {0: "batch", 1: "sequence"},
                "embeddings": {0: "batch"},
            },
            export_params=True,
            external_data=True,
            do_constant_folding=True,
        )
    export_ms = (time.perf_counter() - export_started) * 1000

    model_proto = onnx.load(output_path, load_external_data=False)
    summary = {
        "model": args.model,
        "output": str(output_path),
        "load_ms": round(load_ms, 1),
        "export_ms": round(export_ms, 1),
        "graph_nodes": len(model_proto.graph.node),
        "initializers": len(model_proto.graph.initializer),
        "onnx_bytes": output_path.stat().st_size,
        "sidecars": [
            {"name": path.name, "bytes": path.stat().st_size}
            for path in sorted(output_path.parent.glob(f"{output_path.name}*"))
            if path != output_path
        ],
    }
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
