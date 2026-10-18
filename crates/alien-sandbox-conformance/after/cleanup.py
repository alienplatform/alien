"""Retry termination for provider IDs; report creates without a returned ID."""

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
        if record["event"] == "createRequested":
            runner.intents[record["attemptId"]] = record
        elif record["event"] == "created":
            runner.intents.pop(record["attemptId"], None)
            runner.sessions.append((Sandbox(record["binding"]), record))
        else:
            raise ValueError("unknown journal event")
    outcomes = await runner.cleanup()
    print(
        json.dumps(
            {
                "cleanup": outcomes,
                "unresolvedCreateIntents": list(runner.intents.values()),
            },
            indent=2,
        )
    )
    return (
        0
        if not runner.intents
        and all(item["terminationCallCompleted"] for item in outcomes)
        else 1
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("journal", type=Path)
    sys.exit(asyncio.run(cleanup(parser.parse_args().journal)))
