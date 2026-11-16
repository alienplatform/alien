#!/usr/bin/env python3
"""Print the order in which workspace crates are published to crates.io.

Usage: cargo metadata --no-deps --format-version 1 | crate-publish-order.py CRATE...

The output is the named crates plus every workspace crate they depend on
(normal and build dependencies), one per line. Each crate comes after its own
workspace dependencies. A crate cannot be packaged until the exact version of
each workspace dependency exists on crates.io, so a dependency left out of the
list, or listed after its dependent, fails the publish.

Dev-dependencies are not followed: cargo drops them from the published
manifest. The unpublished `workspace-hack` crate is skipped. A crate in the
result that is marked `publish = false` is an error.
"""

import json
import sys

SKIPPED = {"workspace-hack"}


def publish_order(metadata, roots):
    packages = {
        package["name"]: package
        for package in metadata["packages"]
        if package["id"] in set(metadata["workspace_members"])
    }
    unknown = [root for root in roots if root not in packages]
    if unknown:
        raise SystemExit(f"not workspace crates: {', '.join(unknown)}")

    def workspace_dependencies(name):
        return sorted(
            {
                dependency["name"]
                for dependency in packages[name]["dependencies"]
                if dependency["name"] in packages
                and dependency["name"] not in SKIPPED
                and dependency["kind"] != "dev"
            }
        )

    order, done, visiting = [], set(), []

    def visit(name):
        if name in done:
            return
        if name in visiting:
            cycle = " -> ".join(visiting[visiting.index(name):] + [name])
            raise SystemExit(f"workspace dependency cycle: {cycle}")
        visiting.append(name)
        for dependency in workspace_dependencies(name):
            visit(dependency)
        visiting.pop()
        done.add(name)
        order.append(name)

    for root in roots:
        visit(root)

    unpublishable = [name for name in order if packages[name].get("publish") == []]
    if unpublishable:
        raise SystemExit(
            "these crates are published or are dependencies of published crates, "
            f"but are marked publish = false: {', '.join(unpublishable)}"
        )
    return order


if __name__ == "__main__":
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    print("\n".join(publish_order(json.load(sys.stdin), sys.argv[1:])))
