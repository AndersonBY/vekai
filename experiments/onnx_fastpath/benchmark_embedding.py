import argparse
import ctypes
import importlib.util
import json
import os
import statistics
import time
from pathlib import Path

import numpy as np
import torch
import onnxruntime as ort
from transformers import AutoTokenizer


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


def preload_tensorrt_dlls() -> tuple[list[object], list[object]]:
    if os.name != "nt":
        return [], []

    spec = importlib.util.find_spec("tensorrt")
    if spec is None or spec.origin is None:
        raise RuntimeError("TensorRT Python package is not installed")

    site_packages = Path(spec.origin).resolve().parent.parent
    candidates = (
        site_packages / "tensorrt_libs",
        site_packages / "nvidia" / "cuda_runtime" / "bin",
    )
    directory_handles = [
        os.add_dll_directory(str(directory))
        for directory in candidates
        if directory.is_dir()
    ]
    library_handles = [
        ctypes.WinDLL(str(site_packages / "tensorrt_libs" / name))
        for name in (
            "nvinfer_10.dll",
            "nvinfer_builder_resource_10.dll",
            "nvinfer_plugin_10.dll",
            "nvonnxparser_10.dll",
        )
    ]
    return directory_handles, library_handles


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--hf-model", default="Qwen/Qwen3-Embedding-0.6B")
    parser.add_argument("--counts", default="1,8,32,64")
    parser.add_argument("--sequence-length", type=int, default=32)
    parser.add_argument("--warmup", type=int, default=3)
    parser.add_argument("--samples", type=int, default=10)
    parser.add_argument("--optimized-model", default="")
    parser.add_argument("--dump-embeddings", default="")
    parser.add_argument("--cuda-graph", action="store_true")
    parser.add_argument("--provider", choices=("cuda", "tensorrt"), default="cuda")
    parser.add_argument("--trt-cache-dir", default="")
    parser.add_argument("--profile", default="")
    args = parser.parse_args()

    tokenizer = AutoTokenizer.from_pretrained(args.hf_model, padding_side="left")
    options = ort.SessionOptions()
    options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    if args.profile:
        Path(args.profile).resolve().parent.mkdir(parents=True, exist_ok=True)
        options.enable_profiling = True
        options.profile_file_prefix = str(Path(args.profile).resolve())
    if args.optimized_model:
        options.optimized_model_filepath = args.optimized_model

    cuda_provider_options = {
        "device_id": 0,
        "arena_extend_strategy": "kSameAsRequested",
        "cudnn_conv_algo_search": "HEURISTIC",
        "do_copy_in_default_stream": True,
    }
    if args.cuda_graph:
        cuda_provider_options["enable_cuda_graph"] = True
    providers = []
    dll_handles = ([], [])
    if args.provider == "tensorrt":
        dll_handles = preload_tensorrt_dlls()
        cache_dir = Path(args.trt_cache_dir or ".ort-tensorrt-cache").resolve()
        cache_dir.mkdir(parents=True, exist_ok=True)
        tensorrt_provider_options = {
            "device_id": 0,
            "trt_fp16_enable": True,
            "trt_layer_norm_fp32_fallback": True,
            "trt_engine_cache_enable": True,
            "trt_engine_cache_path": str(cache_dir),
            "trt_timing_cache_enable": True,
            "trt_timing_cache_path": str(cache_dir),
            "trt_build_heuristics_enable": True,
            "trt_profile_min_shapes": "input_ids:1x32,attention_mask:1x32",
            "trt_profile_opt_shapes": "input_ids:32x32,attention_mask:32x32",
            "trt_profile_max_shapes": "input_ids:64x32,attention_mask:64x32",
        }
        providers.append(("TensorrtExecutionProvider", tensorrt_provider_options))
    providers.extend(
        [
            ("CUDAExecutionProvider", cuda_provider_options),
            "CPUExecutionProvider",
        ]
    )
    load_started = time.perf_counter()
    session = ort.InferenceSession(args.model, sess_options=options, providers=providers)
    load_ms = (time.perf_counter() - load_started) * 1000
    expected_provider = (
        "TensorrtExecutionProvider"
        if args.provider == "tensorrt"
        else "CUDAExecutionProvider"
    )
    if session.get_providers()[0] != expected_provider:
        raise RuntimeError(
            f"{expected_provider} is not active: {session.get_providers()}"
        )
    output_type = session.get_outputs()[0].type
    output_dtype = {
        "tensor(float)": np.float32,
        "tensor(float16)": np.float16,
    }.get(output_type)
    if output_dtype is None:
        raise RuntimeError(f"unsupported embeddings output type: {output_type}")

    results = []
    last_embeddings = None
    for count in (int(value) for value in args.counts.split(",")):
        encoded = tokenizer(
            make_inputs(count),
            padding="max_length",
            truncation=True,
            max_length=args.sequence_length,
            return_tensors="np",
        )
        feeds = {
            "input_ids": encoded["input_ids"].astype(np.int64, copy=False),
            "attention_mask": encoded["attention_mask"].astype(np.int64, copy=False),
        }

        if args.cuda_graph:
            input_ids_value = ort.OrtValue.ortvalue_from_numpy(
                feeds["input_ids"], "cuda", 0
            )
            attention_mask_value = ort.OrtValue.ortvalue_from_numpy(
                feeds["attention_mask"], "cuda", 0
            )
            output_value = ort.OrtValue.ortvalue_from_shape_and_type(
                [count, 1024], output_dtype, "cuda", 0
            )
            io_binding = session.io_binding()
            io_binding.bind_ortvalue_input("input_ids", input_ids_value)
            io_binding.bind_ortvalue_input("attention_mask", attention_mask_value)
            io_binding.bind_ortvalue_output("embeddings", output_value)
            run_options = ort.RunOptions()
            run_options.add_run_config_entry("gpu_graph_id", str(count))

            def run() -> np.ndarray:
                input_ids_value.update_inplace(feeds["input_ids"])
                attention_mask_value.update_inplace(feeds["attention_mask"])
                session.run_with_iobinding(io_binding, run_options)
                return output_value.numpy()

        else:

            def run() -> np.ndarray:
                return session.run(["embeddings"], feeds)[0]

        for _ in range(args.warmup):
            run()

        samples = []
        for _ in range(args.samples):
            started = time.perf_counter()
            last_embeddings = run()
            samples.append((time.perf_counter() - started) * 1000)
        if not np.isfinite(last_embeddings).all():
            raise RuntimeError(f"non-finite embeddings returned for count={count}")

        results.append(
            {
                "count": count,
                "p50_ms": round(percentile(samples, 0.5), 1),
                "p90_ms": round(percentile(samples, 0.9), 1),
                "mean_ms": round(statistics.mean(samples), 1),
            }
        )

    if args.dump_embeddings and last_embeddings is not None:
        with open(args.dump_embeddings, "w", encoding="utf-8") as handle:
            json.dump(last_embeddings.astype(np.float32).tolist(), handle)

    profile_path = session.end_profiling() if args.profile else ""
    provider_nodes = {}
    if profile_path:
        with open(profile_path, encoding="utf-8") as handle:
            profile = json.load(handle)
        for event in profile:
            if event.get("cat") != "Node":
                continue
            provider = event.get("args", {}).get("provider")
            if provider:
                provider_nodes[provider] = provider_nodes.get(provider, 0) + 1

    print(
        json.dumps(
            {
                "providers": session.get_providers(),
                "requested_provider": args.provider,
                "output_type": output_type,
                "cuda_graph": args.cuda_graph,
                "load_ms": round(load_ms, 1),
                "profile": profile_path,
                "provider_nodes": provider_nodes,
                "results": results,
            },
            indent=2,
        )
    )
    del dll_handles


if __name__ == "__main__":
    main()
