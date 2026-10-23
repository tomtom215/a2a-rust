#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Fail when a published crate ships an AI model or depends on a runtime for one.

The control map's AIA-53 row (EU AI Act Article 53, obligations of providers
of general-purpose AI models) rests on one claim: the SDK is not a
general-purpose AI model and contains none (Article 3(63)). A claim like that
decays silently. A test fixture could add a small GGUF model, or a feature
could pull in an inference runtime, and nothing would say the row had become
false. This checks what is actually published.

What it checks, for each crate published from this repository:

  * the files `cargo package --list` would upload: none may have a model
    weight or model-format extension, and none may exceed 10 MiB (model
    weights are large; a 10 MiB source file is not);
  * the crate's dependency graph with every feature on (`cargo tree`, normal
    and build edges): no crate that runs or loads models.

Usage:
    check_no_model_artifacts.py             # check the published crates
    check_no_model_artifacts.py --self-test # check that the checks can fail

Exit 0 when nothing is found, 1 when something is, 2 when a cargo command
fails.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# (cargo arguments selecting the crate, the crate name for messages)
PUBLISHED = [
    (["-p", "a2a-protocol-types"], "a2a-protocol-types"),
    (["-p", "a2a-protocol-client"], "a2a-protocol-client"),
    (["-p", "a2a-protocol-server"], "a2a-protocol-server"),
    (["-p", "a2a-protocol-sdk"], "a2a-protocol-sdk"),
    (["--manifest-path", "bindings/a2a-protocol-slimrpc/Cargo.toml"], "a2a-protocol-slimrpc"),
]

# Extensions model weights and model graphs are distributed in.
MODEL_EXTENSIONS = {
    ".gguf", ".ggml", ".safetensors", ".onnx", ".pt", ".pth", ".ckpt",
    ".h5", ".tflite", ".mlmodel", ".pb", ".bin", ".npz", ".llamafile",
}
MAX_BYTES = 10 * 1024 * 1024

# Crates that load or run models. A dependency on one means the SDK could
# carry or execute a model itself rather than call one over a network API.
RUNTIMES = {
    "candle-core", "candle-transformers", "candle-nn", "llama-cpp-2",
    "llama_cpp", "llama-cpp-sys-2", "ort", "ort-sys", "tch", "torch-sys",
    "burn", "burn-core", "mistralrs", "mistralrs-core", "rust-bert",
    "tract-onnx", "tract-core", "tensorflow", "onnxruntime", "whisper-rs",
    "rten", "kalosm", "llm", "ggml", "ggml-sys",
}


def run(args: list[str]) -> str:
    """`args` run at the repository root; its stdout."""
    done = subprocess.run(args, cwd=ROOT, capture_output=True, text=True)
    if done.returncode != 0:
        sys.stderr.write(f"{' '.join(args)} failed:\n{done.stderr}")
        sys.exit(2)
    return done.stdout


def packaged_files(select: list[str]) -> list[str]:
    """The files `cargo package` would upload for the crate `select` names."""
    return [
        line.strip()
        for line in run(["cargo", "package", "--list", "--allow-dirty", *select]).splitlines()
        if line.strip()
    ]


def crate_root(select: list[str]) -> Path:
    """The directory of the crate `select` names."""
    if select[0] == "--manifest-path":
        return ROOT / Path(select[1]).parent
    return ROOT / "crates" / select[1]


def dependencies(select: list[str]) -> set[str]:
    """Every crate in the graph of the crate `select` names, all features on."""
    out = run([
        "cargo", "tree", *select, "--all-features", "-e", "normal,build",
        "--prefix", "none", "--format", "{p}",
    ])
    return {line.split()[0] for line in out.splitlines() if line.strip()}


def file_problems(name: str, files: list[str], sizes: dict[str, int]) -> list[str]:
    """What is wrong with the packaged `files` of crate `name`."""
    problems = []
    for f in files:
        suffix = Path(f).suffix.lower()
        if suffix in MODEL_EXTENSIONS:
            problems.append(f"{name}: packages {f}, a model-format file ({suffix})")
        size = sizes.get(f, 0)
        if size > MAX_BYTES:
            problems.append(f"{name}: packages {f}, {size} bytes (over {MAX_BYTES})")
    return problems


def dependency_problems(name: str, deps: set[str]) -> list[str]:
    """The model runtimes among crate `name`'s dependencies."""
    return [f"{name}: depends on {d}, which loads or runs models" for d in sorted(deps & RUNTIMES)]


def self_test() -> None:
    """Each check reports what it exists to catch, and passes what is clean."""
    files = ["src/lib.rs", "tests/fixtures/tiny.gguf", "assets/blob.dat", "Cargo.toml"]
    sizes = {"assets/blob.dat": MAX_BYTES + 1}
    found = file_problems("x", files, sizes)
    assert any("tiny.gguf" in p for p in found), found
    assert any("blob.dat" in p for p in found), found
    assert file_problems("x", ["src/lib.rs", "README.md"], {"src/lib.rs": 5000}) == []
    assert dependency_problems("x", {"serde", "candle-core"}) == [
        "x: depends on candle-core, which loads or runs models"
    ]
    assert dependency_problems("x", {"serde", "tokio"}) == []
    print("check_no_model_artifacts --self-test: 5 of 5 passed")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--self-test", action="store_true")
    if parser.parse_args().self_test:
        self_test()
        return 0
    self_test()
    problems: list[str] = []
    files_seen = 0
    for select, name in PUBLISHED:
        files = packaged_files(select)
        root = crate_root(select)
        sizes = {f: (root / f).stat().st_size for f in files if (root / f).is_file()}
        files_seen += len(files)
        problems += file_problems(name, files, sizes)
        problems += dependency_problems(name, dependencies(select))
    if problems:
        print("check_no_model_artifacts: a published crate carries or runs a model:")
        for p in problems:
            print(f"  {p}")
        print("The control map's AIA-53 row says the SDK contains no AI model; fix the "
              "crate, or correct the row.")
        return 1
    print(f"check_no_model_artifacts: {len(PUBLISHED)} published crates, {files_seen} "
          "packaged files, no model file and no model runtime in any dependency graph")
    return 0


if __name__ == "__main__":
    sys.exit(main())
