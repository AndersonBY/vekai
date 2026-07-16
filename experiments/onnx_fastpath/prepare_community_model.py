import argparse
from pathlib import Path

import numpy as np
import onnx
from onnx import TensorProto, helper, numpy_helper


def replace_input(graph: onnx.GraphProto, prefix: str, replacement: str) -> None:
    for node in graph.node:
        for index, name in enumerate(node.input):
            if name.startswith(prefix):
                node.input[index] = replacement


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    input_path = Path(args.input).resolve()
    output_path = Path(args.output).resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)

    model = onnx.load(input_path, load_external_data=False)
    graph = model.graph
    if [value.name for value in graph.output[:1]] != ["last_hidden_state"]:
        raise RuntimeError("expected last_hidden_state as the first model output")

    ids_shape = "vekai.input_shape"
    batch_size = "vekai.batch_size"
    sequence_length = "vekai.sequence_length"
    empty_past_shape = "vekai.empty_past_shape"
    empty_past = "vekai.empty_past"
    position_range = "vekai.position_range"
    position_row = "vekai.position_row"
    position_ids = "vekai.position_ids"

    initializers = [
        numpy_helper.from_array(np.array(0, dtype=np.int64), "vekai.index_0"),
        numpy_helper.from_array(np.array(1, dtype=np.int64), "vekai.index_1"),
        numpy_helper.from_array(
            np.array([8, 0, 128], dtype=np.int64), "vekai.empty_past_suffix"
        ),
        numpy_helper.from_array(np.array([0], dtype=np.int64), "vekai.axes_0"),
        numpy_helper.from_array(np.array(0, dtype=np.int64), "vekai.range_start"),
        numpy_helper.from_array(np.array(1, dtype=np.int64), "vekai.range_delta"),
        numpy_helper.from_array(np.array(-1, dtype=np.int64), "vekai.last_index"),
        numpy_helper.from_array(np.array([1e-12], dtype=np.float32), "vekai.norm_epsilon"),
    ]
    graph.initializer.extend(initializers)

    prefix_nodes = [
        helper.make_node("Shape", ["input_ids"], [ids_shape], name="vekai.ShapeInputs"),
        helper.make_node(
            "Gather",
            [ids_shape, "vekai.index_0"],
            [batch_size],
            name="vekai.GatherBatch",
            axis=0,
        ),
        helper.make_node(
            "Gather",
            [ids_shape, "vekai.index_1"],
            [sequence_length],
            name="vekai.GatherSequence",
            axis=0,
        ),
        helper.make_node(
            "Unsqueeze",
            [batch_size, "vekai.axes_0"],
            ["vekai.batch_vector"],
            name="vekai.UnsqueezeBatch",
        ),
        helper.make_node(
            "Concat",
            ["vekai.batch_vector", "vekai.empty_past_suffix"],
            [empty_past_shape],
            name="vekai.EmptyPastShape",
            axis=0,
        ),
        helper.make_node(
            "ConstantOfShape",
            [empty_past_shape],
            [empty_past],
            name="vekai.EmptyPast",
            value=helper.make_tensor("value", TensorProto.FLOAT16, [1], [0.0]),
        ),
        helper.make_node(
            "Range",
            ["vekai.range_start", sequence_length, "vekai.range_delta"],
            [position_range],
            name="vekai.PositionRange",
        ),
        helper.make_node(
            "Unsqueeze",
            [position_range, "vekai.axes_0"],
            [position_row],
            name="vekai.PositionRow",
        ),
        helper.make_node(
            "Expand",
            [position_row, ids_shape],
            [position_ids],
            name="vekai.PositionIds",
        ),
    ]
    existing_nodes = list(graph.node)
    del graph.node[:]
    graph.node.extend(prefix_nodes)
    graph.node.extend(existing_nodes)

    replace_input(graph, "past_key_values.", empty_past)
    replace_input(graph, "position_ids", position_ids)
    retained_inputs = [
        value
        for value in graph.input
        if value.name in {"input_ids", "attention_mask"}
    ]
    del graph.input[:]
    graph.input.extend(retained_inputs)

    graph.node.extend(
        [
            helper.make_node(
                "Gather",
                ["last_hidden_state", "vekai.last_index"],
                ["vekai.last_token"],
                name="vekai.LastTokenPool",
                axis=1,
            ),
            helper.make_node(
                "ReduceL2",
                ["vekai.last_token"],
                ["vekai.norm"],
                name="vekai.L2Norm",
                axes=[1],
                keepdims=1,
            ),
            helper.make_node(
                "Max",
                ["vekai.norm", "vekai.norm_epsilon"],
                ["vekai.safe_norm"],
                name="vekai.SafeNorm",
            ),
            helper.make_node(
                "Div",
                ["vekai.last_token", "vekai.safe_norm"],
                ["embeddings"],
                name="vekai.Normalize",
            ),
        ]
    )
    del graph.output[:]
    graph.output.extend(
        [
            helper.make_tensor_value_info(
                "embeddings", TensorProto.FLOAT, ["batch_size", 1024]
            )
        ]
    )

    onnx.save_model(model, output_path)
    print(output_path)


if __name__ == "__main__":
    main()
