"""Generate the Python Worker protocol from its canonical protobuf source."""

import shutil
import tempfile
from pathlib import Path

import grpc_tools
from grpc_tools import protoc

ROOT = Path(__file__).resolve().parents[3]
OUTPUT = ROOT / "crates/alien-bindings-python/python"
with tempfile.TemporaryDirectory() as directory:
    source = Path(directory) / "alienplatform/_worker_protocol"
    source.mkdir(parents=True)
    shutil.copy(ROOT / "crates/alien-worker-protocol/proto/control.proto", source)
    result = protoc.main(
        [
            "protoc",
            f"-I{directory}",
            f"-I{Path(grpc_tools.__file__).parent / '_proto'}",
            f"--python_out={OUTPUT}",
            f"--pyi_out={OUTPUT}",
            f"--grpc_python_out={OUTPUT}",
            str(source / "control.proto"),
        ]
    )
    if result:
        raise SystemExit(result)
