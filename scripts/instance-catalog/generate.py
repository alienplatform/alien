#!/usr/bin/env python3
"""Generate the instance catalog from the clouds' machine APIs.

Reads crates/alien-core/instance-catalog.toml and writes
crates/alien-core/src/instance_catalog/catalog.rs. From the repository root:
  python3 scripts/instance-catalog/generate.py

Needs the aws, gcloud and az CLIs signed in; it uses their current profile, project and
subscription. vCPU, memory, local disk and GPU numbers come from the APIs, the rest from the
config or the disk constants below, so a rerun on unchanged clouds reproduces the file byte for
byte, and `git diff --exit-code` afterwards shows drift.
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
# AWS family names: series letters, generation, options, then `-flex` or a high-memory size.
# https://docs.aws.amazon.com/ec2/latest/instancetypes/instance-type-names.html
AWS_FAMILY = re.compile(r"^([a-z]+)(\d+)([a-z]*)(-flex|-\d+tb)?$")
AWS_SERIES_CATEGORIES = {
    "t": "Burstable", "m": "GeneralPurpose", "c": "ComputeOptimized",
    "r": "MemoryOptimized", "x": "MemoryOptimized", "z": "MemoryOptimized", "u": "MemoryOptimized",
    "i": "StorageOptimized", "d": "StorageOptimized", "h": "StorageOptimized",
    "im": "StorageOptimized", "is": "StorageOptimized",
}
CATEGORY_LABELS = {
    "Burstable": "Burstable", "GeneralPurpose": "General purpose",
    "ComputeOptimized": "Compute optimized", "MemoryOptimized": "Memory optimized",
    "StorageOptimized": "Storage optimized",
}
# The processor letter of a family, by the manufacturer the API reports. Some Intel families
# have none (m5, z1d, i3en).
AWS_PROCESSOR_LETTERS = {"AWS": "g", "Intel": "i", "AMD": "a"}
# AWS series of a category are listed Graviton first, then AMD, then Intel. This order keeps the
# automatic series in the order automatic selection breaks ties with (r6a before r7i).
AWS_PROCESSOR_ORDER = ("AWS", "AMD", "Intel")
# The processor each Graviton generation runs, from the families' processor column at
# https://docs.aws.amazon.com/ec2/latest/instancetypes/gp.html (and co.html, mo.html, so.html).
GRAVITON = {2: "Graviton2", 4: "Graviton2", 6: "Graviton2", 7: "Graviton3", 8: "Graviton4",
            9: "Graviton5"}
GRAVITON_FAMILIES = {"c7gn": "Graviton3E"}
# Series letters that share a category with others, in the naming page's words, so their labels
# differ from that category's main series (x8g from r8g).
SERIES_WORDS = {"x": "Memory intensive", "z": "High memory", "u": "High memory",
                "d": "Dense storage"}
# Option letters, in the naming page's words. `d` is named from the disk the API reports, and
# `e` means extra storage or extra memory depending on the category.
VARIANT_WORDS = {"b": "Block storage optimized", "n": "Network and EBS optimized",
                 "z": "High CPU frequency"}
EXTRA_WORDS = {"StorageOptimized": "Extra instance storage", "MemoryOptimized": "Extra memory"}
DISK_WORDS = {"nvme": "Local NVMe", "hdd": "Local HDD"}
# Within a processor and generation: the plain family, then -flex, then d, then the other
# variants.
VARIANT_RANK = {"": 0, "-flex": 1, "d": 2}
# The largest gp3 volume. A storage-optimized machine's root volume is sized from its local disk.
AWS_MAX_ROOT_GIB = 65536


def run(command):
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"{' '.join(command)} exited with {result.returncode}:\n"
                           f"{result.stderr.strip()}")
    machines = json.loads(result.stdout)
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


def bootable(raw):
    """Whether the AWS host image boots the machine: x86 boots in UEFI mode, or on bare metal;
    the arm64 image boots on every arm64 machine. Xen virtual machines, Macs and machines with
    accelerators are left out."""
    if raw["InstanceType"].startswith("mac") or any(
            key in raw for key in ("GpuInfo", "NeuronInfo", "InferenceAcceleratorInfo",
                                   "FpgaInfo", "MediaAcceleratorInfo")):
        return False
    if raw.get("Hypervisor") == "xen" and not raw["BareMetal"]:
        return False
    return ("arm64" in raw["ProcessorInfo"]["SupportedArchitectures"]
            or "uefi" in raw["SupportedBootModes"] or raw["BareMetal"])


def aws_disk(storage):
    kinds = {disk["Type"] for disk in storage.get("Disks", [])}
    if not kinds:
        return None
    if kinds == {"ssd"} and storage.get("NvmeSupport") == "required":
        return "nvme"
    return "hdd" if kinds == {"hdd"} else f"{'/'.join(sorted(kinds))} {storage.get('NvmeSupport')}"


def aws_machine(raw):
    storage = raw.get("InstanceStorageInfo") or {}
    gpus = (raw.get("GpuInfo") or {}).get("Gpus") or []
    return raw["InstanceType"], {
        "vcpu": raw["VCpuInfo"]["DefaultVCpus"],
        "memory_mib": raw["MemoryInfo"]["SizeInMiB"],
        "architecture": "Arm64" if "arm64" in raw["ProcessorInfo"]["SupportedArchitectures"]
        else "X86_64",
        # AWS reports local disk in decimal GB.
        "local_mib": storage.get("TotalSizeInGB", 0) * 1000**3 // 1024**2,
        "disk": aws_disk(storage),
        "gpu": (f"{gpus[0]['Manufacturer']}-{gpus[0]['Name']}".lower(), gpus[0]["Count"])
        if gpus else None,
        "nested_virt": "nested-virtualization" in raw["ProcessorInfo"].get("SupportedFeatures", []),
        "metal": raw["BareMetal"],
        "bootable": bootable(raw),
        "current": raw["CurrentGeneration"],
        "manufacturer": raw["ProcessorInfo"].get("Manufacturer"),
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


def aws_family(name, machines):
    """What the family name and the API say about a family (`m8id`, `m8i-flex`)."""
    match = AWS_FAMILY.match(name)
    if match is None:
        raise RuntimeError(f"{name}: not an AWS family name")
    letters, generation, options, suffix = match.groups()
    if letters not in AWS_SERIES_CATEGORIES:
        raise RuntimeError(f"{name}: unknown series letters `{letters}`, add them to "
                           "AWS_SERIES_CATEGORIES")
    members = [machine for machine_name, machine in machines.items()
               if machine_name.split(".")[0] == name]
    facts = {(machine["manufacturer"], machine["current"], machine["disk"]) for machine in members}
    if len(facts) != 1:
        raise RuntimeError(f"{name}: machines differ in processor, generation or disk: {facts}")
    processor, current, disk = facts.pop()
    if processor not in AWS_PROCESSOR_LETTERS:
        raise RuntimeError(f"{name}: unknown processor manufacturer {processor}")
    letter = AWS_PROCESSOR_LETTERS[processor]
    if options.startswith(letter):
        options = options[1:]
    elif processor != "Intel":
        raise RuntimeError(f"{name}: {processor} family without the `{letter}` letter")
    return {
        "category": AWS_SERIES_CATEGORIES[letters],
        "letters": letters,
        "generation": int(generation),
        "processor": processor,
        # The options after the processor letter, or the family's suffix.
        "variant": suffix if suffix else options,
        "options": options,
        "suffix": suffix,
        "current": current,
        "disk": disk,
    }


def ordinal(number):
    if number % 100 in (11, 12, 13):
        return f"{number}th"
    return f"{number}{ {1: 'st', 2: 'nd', 3: 'rd'}.get(number % 10, 'th')}"


def aws_label(name, family, metal):
    category = family["category"]
    if family["processor"] == "AWS":
        if family["generation"] not in GRAVITON:
            raise RuntimeError(f"{name}: no Graviton name for generation {family['generation']}")
        words = [GRAVITON_FAMILIES.get(name, GRAVITON[family["generation"]])]
    else:
        words = [family["processor"], f"{ordinal(family['generation'])} gen"]
    if family["letters"] in SERIES_WORDS:
        words.append(SERIES_WORDS[family["letters"]])
    for letter in family["options"]:
        if letter == "d":
            word = DISK_WORDS.get(family["disk"])
        elif letter == "e":
            word = EXTRA_WORDS.get(category)
        else:
            word = VARIANT_WORDS.get(letter)
        if word is None:
            key = {"d": family["disk"], "e": category}.get(letter, letter)
            table = {"d": "DISK_WORDS", "e": "EXTRA_WORDS"}.get(letter, "VARIANT_WORDS")
            raise RuntimeError(f"{name}: no label word for option `{letter}`; add {key!r} to "
                               f"{table}, or list `{name}` (and `{name}-metal`) in the config")
        words.append(word)
    suffix = family["suffix"]
    if suffix == "-flex":
        words.append("Flex")
    elif suffix:
        words.append(f"{suffix[1:-2]} TB memory")
    if metal:
        words.append("Bare metal")
    return " · ".join([CATEGORY_LABELS[category], *words])


def aws_root_gib(local_mib):
    """The root volume a storage-optimized machine gets, as `aws_root_volume_size_gib` sizes
    it from the local disk."""
    local_gib = -(-local_mib // 1024)
    return -(-local_gib * 5 // 4) + 12


def aws_series(config, machines, offered_everywhere):
    """Every AWS series in catalog order: the config's, and one per family of the machines the
    host image boots in every region that no config series names. Within a category they are
    sorted by processor, generation and variant; GPU series keep the config's order. Each gets
    `primary`."""
    hand = config["aws"]["series"]
    hand_names = {series["series"] for series in hand}
    generated = {}
    for name in sorted(offered_everywhere):
        machine = machines[name]
        if not machine["bootable"] or machine["memory_mib"] < 1024:
            continue
        family_name = name.split(".")[0]
        series_name = f"{family_name}-metal" if machine["metal"] else family_name
        if series_name in hand_names:
            continue
        family = aws_family(family_name, machines)
        if (family["category"] == "StorageOptimized"
                and aws_root_gib(machine["local_mib"]) > AWS_MAX_ROOT_GIB):
            continue
        series = generated.setdefault(series_name, {
            "series": series_name,
            "category": family["category"],
            "label": aws_label(family_name, family, machine["metal"]),
            "sizes": [],
        })
        series["sizes"].append(name)
    for series in generated.values():
        members = [machines[name] for name in series["sizes"]]
        if series["series"].endswith("-metal"):
            # The catalog's nested_virt means an x86_64 KVM guest (row() enforces it). Bare metal
            # gives guests KVM from the hardware, so x86 metal is true and arm64 metal false.
            series["nested_virt"] = all(m["architecture"] == "X86_64" for m in members)
        else:
            nested = {m["nested_virt"] for m in members}
            if len(nested) != 1:
                raise RuntimeError(f"{series['series']}: sizes differ in nested virtualization")
            series["nested_virt"] = nested.pop()

    def facts(series):
        return series["series"].removesuffix("-metal"), series["series"].endswith("-metal")

    families = {}
    for series in [*hand, *generated.values()]:
        if series["category"] != "GpuCompute":
            family_name, _ = facts(series)
            families.setdefault(family_name, aws_family(family_name, machines))
    # Only families whose variant qualifies set the newest generation, so a generation that
    # ships first in another variant does not leave its processor without a primary series.
    primary_variants = ("", "d", "-flex")
    newest = {}
    for family in families.values():
        if family["variant"] in primary_variants:
            key = (family["category"], family["processor"])
            newest[key] = max(newest.get(key, 0), family["generation"])

    def primary(series):
        if series["category"] == "GpuCompute":
            return True
        family = families[facts(series)[0]]
        return (family["current"] and family["variant"] in primary_variants
                and family["generation"] == newest[(family["category"], family["processor"])])

    def order(series):
        family_name, metal = facts(series)
        family = families[family_name]
        return (AWS_PROCESSOR_ORDER.index(family["processor"]), family["generation"],
                VARIANT_RANK.get(family["variant"], len(VARIANT_RANK)), family_name, metal)

    categories = {}
    for series in [*hand, *generated.values()]:
        categories.setdefault(series["category"], []).append(series)
    # Categories come from the config's order; a family the letter table puts in a category
    # the config has no AWS series for would have no place in it.
    unknown = sorted(set(categories) - {series["category"] for series in hand})
    if unknown:
        raise RuntimeError(f"generated AWS series in categories no config series has: {unknown}")
    ordered = []
    for category, members in categories.items():
        ordered += members if category == "GpuCompute" else sorted(members, key=order)
    return [{**series, "primary": primary(series)} for series in ordered]


def row(platform, series, name, machine_type, machine, auto_select):
    category = series["category"]
    if category not in CATEGORIES:
        raise RuntimeError(f"{platform} {series['series']}: unknown category {category}")
    override = series.get("overrides", {}).get(name, {})
    nested_virt = override.get("nested_virt", series.get("nested_virt", False))
    # Bare metal gives guests KVM from the hardware; AWS reports nested virtualization only
    # for virtual machines.
    if nested_virt and platform == "aws" and not (machine["nested_virt"] or machine["metal"]):
        raise RuntimeError(f"{name} is flagged nested_virt but AWS does not report it")
    if nested_virt and machine["metal"] and machine["architecture"] != "X86_64":
        raise RuntimeError(f"{name}: nested virtualization on AWS means x86_64")
    if machine["metal"] and auto_select:
        raise RuntimeError(f"{name} is bare metal, which automatic selection never picks")
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
    if platform == "aws" and aws_root_gib(storage_mib) > AWS_MAX_ROOT_GIB:
        raise RuntimeError(f"{name}: a {aws_root_gib(storage_mib)} GiB root volume exceeds the "
                           f"largest gp3 volume ({AWS_MAX_ROOT_GIB} GiB); end the series before "
                           "it with `to`, or drop it from `sizes` or `overrides`")
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
        "local_disk_mib": machine["local_mib"],
        "machine_type": machine_type,
        "gpu": gpu,
        "nested_virt": nested_virt,
        "bare_metal": machine["metal"],
        "auto_select": auto_select,
        "primary": series["primary"],
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
                                       bytes_expression(spec["storage_mib"]),
                                       bytes_expression(spec["local_disk_mib"]))} - {"0"})
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
            f"        local_disk_bytes: {bytes_expression(spec['local_disk_mib'])},",
            f'        machine_type: "{spec["machine_type"]}",',
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
            f"        bare_metal: {str(spec['bare_metal']).lower()},",
            f"        auto_select: {str(spec['auto_select']).lower()},",
            f"        primary: {str(spec['primary']).lower()},",
            "    },",
        ]
    lines.append("];")
    return "\n".join(lines) + "\n"


def all_primary(series_list):
    """GCP and Azure list only chosen series, so every one is primary."""
    return [{**series, "primary": True} for series in series_list]


def platform_rows(platform, series_list, machines):
    rows = []
    for series in series_list:
        members = series_machines(platform, series, machines)
        check_overrides(platform, series, members)
        for name, machine, auto_select in members:
            rows.append(row(platform, series, name, machine, machines[machine], auto_select))
    return rows


def main():
    config = tomllib.loads(CONFIG.read_text())
    with ThreadPoolExecutor() as pool:
        aws = [pool.submit(aws_machines, region) for region in config["aws"]["regions"]]
        gcp = pool.submit(gcp_machines, config["gcp"]["zones"])
        azure = [pool.submit(azure_machines, region) for region in config["azure"]["regions"]]
        aws_regions = [future.result() for future in aws]
        machines = {
            "aws": merge(aws_regions, aws_machine),
            "gcp": merge([gcp.result()], gcp_machine),
            "azure": merge([future.result() for future in azure], azure_machine),
        }
    offered_everywhere = set.intersection(*({raw["InstanceType"] for raw in region}
                                            for region in aws_regions))
    rows = (platform_rows("aws", aws_series(config, machines["aws"], offered_everywhere),
                          machines["aws"])
            + platform_rows("gcp", all_primary(config["gcp"]["series"]), machines["gcp"])
            + platform_rows("azure", all_primary(config["azure"]["series"]), machines["azure"]))
    names = [(spec["platform"], spec["name"]) for spec in rows]
    duplicates = sorted({name for name in names if names.count(name) > 1})
    if duplicates:
        raise RuntimeError(f"machines listed twice: {duplicates}")
    OUTPUT.write_text(render(rows))
    print(f"Wrote {len(rows)} machines to {OUTPUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
