"""Retry termination for every persisted create intent, including failed creates."""

import argparse
import asyncio
import json
import sys
from pathlib import Path

from run import Run


async def cleanup(path):
    from alienplatform import Sandbox

    runner = Run({}, None, Sandbox)
    for line in path.read_text().splitlines():
        record = json.loads(line)
        runner.sessions.append((Sandbox(record["binding"]), record))
    outcomes = await runner.cleanup()
    print(json.dumps(outcomes, indent=2))
    return 0 if all(item["terminationCallCompleted"] for item in outcomes) else 1


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("journal", type=Path)
    sys.exit(asyncio.run(cleanup(parser.parse_args().journal)))
