"""Loads the probe script under test (CNI_PROBE_SOURCE overrides the path, for before/after runs)."""

import importlib.util
import os
import pathlib

SOURCE = pathlib.Path(os.environ.get(
    "CNI_PROBE_SOURCE",
    pathlib.Path(__file__).parents[2] / "src" / "emitters" / "sandbox_cni_probe.py"))
spec = importlib.util.spec_from_file_location("sandbox_cni_probe", SOURCE)
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)
