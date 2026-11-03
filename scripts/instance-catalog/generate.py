#!/usr/bin/env python3
"""Generate the instance catalog from the clouds' machine APIs.

Reads crates/alien-core/instance-catalog.toml and writes
crates/alien-core/src/instance_catalog/catalog.rs. From the repository root:
  python3 scripts/instance-catalog/generate.py

Needs the aws, gcloud and az CLIs signed in; it uses their current profile, project and
subscription. Every number in the output comes from the APIs, so a rerun on unchanged clouds
reproduces the file byte for byte, and `git diff --exit-code` afterwards shows drift.
"""

from concurrent.futures import ThreadPoolExecutor
from decimal import Decimal
import json
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[2]
CONFIG = ROOT / "crates/alien-core/instance-catalog.toml"
OUTPUT = ROOT / "crates/alien-core/src/instance_catalog/catalog.rs"

CATEGORIES = ("Burstable", "GeneralPurpose", "ComputeOptimized", "MemoryOptimized",
              "StorageOptimized", "GpuCompute")
# Controllers size a provider disk to the workload, so a machine without fixed local storage
# lists the disk a workload gets when it asks for none.
CONFIGURABLE_DISK_GIB = 20
# A GPU machine's disk is not resized to the workload, so one without local storage lists the
# disk its controllers provision.
GPU_DISK_GIB = 100
# GCP reports how many local SSD disks a machine bundles, not their size; the bundled series
# listed in the config use 375 GiB disks. https://docs.cloud.google.com/compute/docs/disks/local-ssd
GCP_LOCAL_SSD_PARTITION_GIB = 375
# GCP accelerator names, mapped to the GPU types workloads request.
GCP_GPU_TYPES = {
    "nvidia-tesla-t4": "nvidia-t4",
    "nvidia-tesla-a100": "nvidia-a100",
    "nvidia-h100-80gb": "nvidia-h100",
}
AWS_SIZE = r"(?:nano|micro|small|medium|large|\d*xlarge)"


def run(command):
    output = subprocess.run(command, check=True, capture_output=True, text=True).stdout
    machines = json.loads(output)
    if not machines:
        raise RuntimeError(f"{' '.join(command)} returned no machines")
    return machines


def aws_machines(region):
    return run(["aws", "ec2", "describe-instance-types", "--region", region,
                "--output", "json"])["InstanceTypes"]


def gcp_machines(zones):
    return run(["gcloud", "compute", "machine-types", "list",
                f"--filter=zone:({' '.join(zones)})", "--format=json"])


def azure_machines(region):
    # --all keeps sizes the signed-in subscription is restricted from, so the output does not
    # depend on which subscription runs it.
    return run(["az", "vm", "list-skus", "--location", region, "--all",
                "--resource-type", "virtualMachines", "--output", "json"])


def aws_machine(raw):
    storage = raw.get("InstanceStorageInfo") or {}
    gpus = (raw.get("GpuInfo") or {}).get("Gpus") or []
    return raw["InstanceType"], {
        "vcpu": raw["VCpuInfo"]["DefaultVCpus"],
        "memory_mib": raw["MemoryInfo"]["SizeInMiB"],
        "architecture": "Arm64" if "arm64" in raw["ProcessorInfo"]["SupportedArchitectures"]
        else "X86_64",
        # AWS reports decimal GB. The catalog reads the figure as GiB, which overstates the disk
        # by about 7%; the pinned AWS local-disk rows use that reading, so a storage fit decided
        # on them stays the same.
        "local_mib": storage.get("TotalSizeInGB", 0) * 1024,
        "gpu": (f"{gpus[0]['Manufacturer']}-{gpus[0]['Name']}".lower(), gpus[0]["Count"])
        if gpus else None,
        "nested_virt": "nested-virtualization" in raw["ProcessorInfo"].get("SupportedFeatures", []),
        "metal": raw["BareMetal"],
    }


def gcp_machine(raw):
    accelerators = raw.get("accelerators") or []
    return raw["name"], {
        "vcpu": raw["guestCpus"],
        "memory_mib": raw["memoryMb"],
        # Older series omit the field; all of them are x86.
        "architecture": "Arm64" if raw.get("architecture") == "ARM64" else "X86_64",
        "local_mib": raw.get("bundledLocalSsds", {}).get("partitionCount", 0)
        * GCP_LOCAL_SSD_PARTITION_GIB * 1024,
        "gpu": (accelerators[0]["guestAcceleratorType"], accelerators[0]["guestAcceleratorCount"])
        if accelerators else None,
        "metal": False,
    }


def azure_machine(raw):
    capabilities = {capability["name"]: capability["value"]
                    for capability in raw.get("capabilities", [])}
    memory_mib = Decimal(capabilities["MemoryGB"]) * 1024
    if memory_mib != int(memory_mib):
        raise RuntimeError(f"{raw['name']} reports {capabilities['MemoryGB']} GB of memory")
    # NVMe disks are the fixed local storage where a size has them; otherwise the temp disk is.
    local_mib = int(capabilities.get("NvmeDiskSizeInMiB")
                    or capabilities.get("MaxResourceVolumeMB") or 0)
    return raw["name"], {
        "vcpu": int(capabilities["vCPUs"]),
        "memory_mib": int(memory_mib),
        "architecture": "Arm64" if capabilities["CpuArchitectureType"] == "Arm64" else "X86_64",
        "local_mib": local_mib,
        "gpu_count": int(capabilities.get("GPUs", 0)),
        "metal": False,
    }


def merge(locations, parse):
    """One entry per machine name, failing when two locations disagree on what it is."""
    machines = {}
    for raw_machines in locations:
        for raw in raw_machines:
            name, machine = parse(raw)
            if machines.setdefault(name, machine) != machine:
                raise RuntimeError(f"{name} differs between locations: "
                                   f"{machines[name]} != {machine}")
    return machines


def series_machines(platform, series, machines):
    if "sizes" in series:
        names = [(name, series.get("machine", name)) for name in series["sizes"]]
        for name, machine in names:
            if machine not in machines:
                raise RuntimeError(f"{platform} {name}: {machine} is not offered")
    else:
        default = {"aws": "{series}.{size}", "gcp": "{series}-{n}"}.get(platform)
        pattern = series.get("pattern", default)
        if pattern is None:
            raise RuntimeError(f"{platform} {series['series']} needs a pattern")
        tokens = {"{series}": re.escape(series["series"]), "{n}": r"\d+", "{size}": AWS_SIZE}
        regex = re.compile("^" + "".join(tokens.get(part, re.escape(part))
                                         for part in re.split(r"(\{\w+\})", pattern)) + "$")
        names = [(name, name) for name in machines
                 if regex.match(name) and not machines[name]["metal"]]
    names.sort(key=lambda pair: (machines[pair[1]]["vcpu"], machines[pair[1]]["memory_mib"],
                                 pair[0]))
    ordered = [name for name, _ in names]
    if "sizes" not in series:
        bounds = [series["from"], series.get("to", ordered[-1] if ordered else None)]
        for bound in bounds:
            if bound not in ordered:
                raise RuntimeError(f"{platform} {series['series']}: {bound} is not offered")
        names = names[ordered.index(bounds[0]):ordered.index(bounds[1]) + 1]
        ordered = [name for name, _ in names]
    auto_select_to = series.get("auto_select_to")
    if auto_select_to is not None and auto_select_to not in ordered:
        raise RuntimeError(f"{platform} {series['series']}: auto_select_to {auto_select_to} "
                           "is not in the series")
    last_auto = ordered.index(auto_select_to) if auto_select_to else -1
    return [(name, machine, index <= last_auto) for index, (name, machine) in enumerate(names)]


def gcp_gpu_type(accelerator):
    if accelerator not in GCP_GPU_TYPES:
        raise RuntimeError(f"GCP accelerator {accelerator} has no GPU type in GCP_GPU_TYPES")
    return GCP_GPU_TYPES[accelerator]


def check_overrides(platform, series, members):
    """Overrides keep automatic picks unchanged, so each must name one and a known field."""
    automatic = {name for name, _, auto_select in members if auto_select}
    for name, fields in series.get("overrides", {}).items():
        if name not in automatic:
            raise RuntimeError(f"{platform} {series['series']}: override {name} is not an "
                               "automatic pick of the series")
        unknown = set(fields) - {"memory_gib", "ephemeral_storage_gib", "nested_virt"}
        if unknown:
            raise RuntimeError(f"{platform} {name}: unknown override fields {sorted(unknown)}")


def row(platform, series, name, machine, auto_select):
    category = series["category"]
    if category not in CATEGORIES:
        raise RuntimeError(f"{platform} {series['series']}: unknown category {category}")
    override = series.get("overrides", {}).get(name, {})
    nested_virt = override.get("nested_virt", series.get("nested_virt", False))
    if nested_virt and platform == "aws" and not machine["nested_virt"]:
        raise RuntimeError(f"{name} is flagged nested_virt but AWS does not report it")
    if platform == "azure":
        gpu = (series["gpu_type"], machine["gpu_count"]) if machine["gpu_count"] else None
    elif "attached_gpu" in series:
        gpu = (gcp_gpu_type(series["attached_gpu"]), 1)
    elif platform == "gcp" and machine["gpu"]:
        gpu = (gcp_gpu_type(machine["gpu"][0]), machine["gpu"][1])
    else:
        gpu = machine["gpu"]
    if (gpu is not None) != (category == "GpuCompute"):
        raise RuntimeError(f"{name}: GPU {gpu} does not fit category {category}")
    if "ephemeral_storage_gib" in override:
        storage_mib = override["ephemeral_storage_gib"] * 1024
    elif category == "StorageOptimized":
        storage_mib = machine["local_mib"]
    elif category == "GpuCompute":
        storage_mib = machine["local_mib"] or GPU_DISK_GIB * 1024
    else:
        storage_mib = CONFIGURABLE_DISK_GIB * 1024
    return {
        "name": name,
        "platform": platform.capitalize(),
        "family": category,
        "series": series["series"],
        "label": series["label"],
        "architecture": machine["architecture"],
        "vcpu": machine["vcpu"],
        "memory_mib": override["memory_gib"] * 1024 if "memory_gib" in override
        else machine["memory_mib"],
        "storage_mib": storage_mib,
        "gpu": gpu,
        "nested_virt": nested_virt,
        "auto_select": auto_select,
    }


def bytes_expression(mib):
    if mib == 0:
        return "0"
    if mib == 1024:
        return "GI"
    return f"{mib // 1024} * GI" if mib % 1024 == 0 else f"{mib} * MI"


def render(rows):
    lines = [
        "// Generated by `python3 scripts/instance-catalog/generate.py` from",
        "// `crates/alien-core/instance-catalog.toml`. Do not edit by hand.",
        "",
    ]
    units = sorted({expression.split(" * ")[-1] for spec in rows
                    for expression in (bytes_expression(spec["memory_mib"]),
                                       bytes_expression(spec["storage_mib"]))} - {"0"})
    imports = ["Architecture", "CatalogGpu", "InstanceFamily", "InstanceTypeSpec", *units]
    lines += [f"use super::{{{', '.join(imports)}}};", "use crate::Platform;", "",
              "pub(super) static CATALOG: &[InstanceTypeSpec] = &["]
    for spec in rows:
        lines += [
            "    InstanceTypeSpec {",
            f'        name: "{spec["name"]}",',
            f"        platform: Platform::{spec['platform']},",
            f"        family: InstanceFamily::{spec['family']},",
            f'        series: "{spec["series"]}",',
            f'        label: "{spec["label"]}",',
            f"        architecture: Architecture::{spec['architecture']},",
            f"        vcpu: {spec['vcpu']},",
            f"        memory_bytes: {bytes_expression(spec['memory_mib'])},",
            f"        ephemeral_storage_bytes: {bytes_expression(spec['storage_mib'])},",
        ]
        if spec["gpu"] is None:
            lines.append("        gpu: None,")
        else:
            lines += [
                "        gpu: Some(CatalogGpu {",
                f'            gpu_type: "{spec["gpu"][0]}",',
                f"            count: {spec['gpu'][1]},",
                "        }),",
            ]
        lines += [
            f"        nested_virt: {str(spec['nested_virt']).lower()},",
            f"        auto_select: {str(spec['auto_select']).lower()},",
            "    },",
        ]
    lines.append("];")
    return "\n".join(lines) + "\n"


def main():
    config = tomllib.loads(CONFIG.read_text())
    with ThreadPoolExecutor() as pool:
        aws = [pool.submit(aws_machines, region) for region in config["aws"]["regions"]]
        gcp = pool.submit(gcp_machines, config["gcp"]["zones"])
        azure = [pool.submit(azure_machines, region) for region in config["azure"]["regions"]]
        machines = {
            "aws": merge([future.result() for future in aws], aws_machine),
            "gcp": merge([gcp.result()], gcp_machine),
            "azure": merge([future.result() for future in azure], azure_machine),
        }
    rows = []
    for platform in ("aws", "gcp", "azure"):
        for series in config[platform]["series"]:
            members = series_machines(platform, series, machines[platform])
            check_overrides(platform, series, members)
            for name, machine, auto_select in members:
                rows.append(row(platform, series, name, machines[platform][machine],
                                auto_select))
    names = [(spec["platform"], spec["name"]) for spec in rows]
    duplicates = sorted({name for name in names if names.count(name) > 1})
    if duplicates:
        raise RuntimeError(f"machines listed twice: {duplicates}")
    OUTPUT.write_text(render(rows))
    print(f"Wrote {len(rows)} machines to {OUTPUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
