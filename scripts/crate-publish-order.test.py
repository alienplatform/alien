#!/usr/bin/env python3
"""Tests for crate-publish-order.py. Run: python3 scripts/crate-publish-order.test.py"""

import importlib.util
import pathlib
import unittest

_spec = importlib.util.spec_from_file_location(
    "crate_publish_order",
    pathlib.Path(__file__).with_name("crate-publish-order.py"),
)
_module = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_module)
publish_order = _module.publish_order


def workspace(crates):
    """crates: {name: (publish, [(dependency, kind), ...])}; kind None is normal."""
    packages = [
        {
            "id": f"{name}-id",
            "name": name,
            "publish": publish,
            "dependencies": [{"name": dep, "kind": kind} for dep, kind in deps],
        }
        for name, (publish, deps) in crates.items()
    ]
    return {"packages": packages, "workspace_members": [p["id"] for p in packages]}


class PublishOrder(unittest.TestCase):
    def test_a_dependency_missing_from_the_named_crates_is_published_first(self):
        metadata = workspace(
            {
                "core": (None, [("error", None), ("signing", None)]),
                "signing": (None, [("error", None)]),
                "error": (None, []),
            }
        )
        self.assertEqual(publish_order(metadata, ["error", "core"]), ["error", "signing", "core"])

    def test_every_crate_comes_after_its_workspace_dependencies(self):
        metadata = workspace(
            {
                "sdk": (None, [("bindings", None), ("core", None)]),
                "bindings": (None, [("core", None)]),
                "core": (None, [("macros", "build")]),
                "macros": (None, []),
            }
        )
        order = publish_order(metadata, ["sdk"])
        self.assertEqual(order, ["macros", "core", "bindings", "sdk"])

    def test_build_dependencies_are_followed_and_dev_dependencies_are_not(self):
        metadata = workspace(
            {
                "core": (None, [("codegen", "build"), ("fixtures", "dev")]),
                "codegen": (None, []),
                "fixtures": ([], []),
            }
        )
        self.assertEqual(publish_order(metadata, ["core"]), ["codegen", "core"])

    def test_the_workspace_hack_crate_is_skipped(self):
        metadata = workspace(
            {
                "core": (None, [("workspace-hack", None)]),
                "workspace-hack": ([], []),
            }
        )
        self.assertEqual(publish_order(metadata, ["core"]), ["core"])

    def test_external_dependencies_are_ignored(self):
        metadata = workspace({"core": (None, [("serde", None)])})
        self.assertEqual(publish_order(metadata, ["core"]), ["core"])

    def test_each_crate_appears_once(self):
        metadata = workspace(
            {
                "a": (None, [("shared", None)]),
                "b": (None, [("shared", None)]),
                "shared": (None, []),
            }
        )
        self.assertEqual(publish_order(metadata, ["a", "b", "shared"]), ["shared", "a", "b"])

    def test_a_dependency_marked_publish_false_is_refused(self):
        metadata = workspace({"core": (None, [("internal", None)]), "internal": ([], [])})
        with self.assertRaisesRegex(SystemExit, "publish = false: internal"):
            publish_order(metadata, ["core"])

    def test_a_cycle_is_refused(self):
        metadata = workspace({"a": (None, [("b", None)]), "b": (None, [("a", None)])})
        with self.assertRaisesRegex(SystemExit, "cycle: a -> b -> a"):
            publish_order(metadata, ["a"])

    def test_an_unknown_crate_is_refused(self):
        metadata = workspace({"core": (None, [])})
        with self.assertRaisesRegex(SystemExit, "not workspace crates: missing"):
            publish_order(metadata, ["missing"])


if __name__ == "__main__":
    unittest.main()
