"""Release regression tests. All registry and publication calls are mocked."""

import io
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import call, patch
from urllib.error import HTTPError, URLError

import publish_crates as release


VERSION = "0.5.0"


def metadata():
    packages = []
    for name in release.CRATES:
        dependencies = []
        if name == "mixtape-core":
            dependencies = [
                {"name": "mixtape-anthropic-sdk", "kind": None},
                {"name": "mixtape-tools", "kind": "dev"},
            ]
        elif name != "mixtape-anthropic-sdk":
            dependencies = [{"name": "mixtape-core", "kind": None}]
        packages.append({
            "id": name,
            "name": name,
            "version": VERSION,
            "publish": None,
            "dependencies": dependencies,
        })
    return {"packages": packages, "workspace_members": list(release.CRATES)}


class WorkspaceTests(unittest.TestCase):
    def validate(self, data, version=VERSION):
        result = subprocess.CompletedProcess([], 0, stdout=json.dumps(data))
        with patch.object(release.subprocess, "run", return_value=result) as run:
            release.validate_workspace(version)
            self.assertIn("--locked", run.call_args.args[0])

    def test_all_six_crates_and_dev_only_cycles(self):
        self.assertEqual(len(release.CRATES), 6)
        self.assertIn("mixtape-acp", release.CRATES)
        self.validate(metadata())

    def test_tag_version_mismatch_stops_release(self):
        data = metadata()
        data["packages"][1]["version"] = "0.4.0"
        with self.assertRaisesRegex(RuntimeError, "does not match release tag"):
            self.validate(data)

    def test_missing_workspace_crate_stops_release(self):
        data = metadata()
        data["packages"].pop()
        with self.assertRaisesRegex(RuntimeError, "every workspace crate"):
            self.validate(data)

    def test_unordered_runtime_dependency_stops_release(self):
        data = metadata()
        data["packages"][0]["dependencies"] = [{"name": "mixtape-core", "kind": None}]
        with self.assertRaisesRegex(RuntimeError, "Publish mixtape-core before"):
            self.validate(data)

    def test_non_publishable_package_stops_release(self):
        data = metadata()
        data["packages"][0]["publish"] = []
        with self.assertRaisesRegex(RuntimeError, "not enabled"):
            self.validate(data)

    def test_invalid_version_never_invokes_cargo(self):
        for version in ["v0.5.0", "0.5.0/path", "0.5.0\n", "0.5.0;true"]:
            with self.subTest(version=version), patch.object(release.subprocess, "run") as run:
                with self.assertRaises(RuntimeError):
                    release.validate_workspace(version)
                run.assert_not_called()


class RegistryTests(unittest.TestCase):
    def test_exact_published_version(self):
        data = {"version": {"crate": "mixtape-core", "num": VERSION, "yanked": False}}
        with patch.object(release, "urlopen", return_value=io.BytesIO(json.dumps(data).encode())) as open_url:
            self.assertTrue(release.version_exists("mixtape-core", VERSION))
            request = open_url.call_args.args[0]
            self.assertEqual(request.full_url, f"https://crates.io/api/v1/crates/mixtape-core/{VERSION}")
            self.assertEqual(open_url.call_args.kwargs["timeout"], 30)

    def test_only_404_means_unpublished(self):
        for status in [401, 403, 404, 429, 500, 503]:
            error = HTTPError("https://crates.io", status, "synthetic", {}, None)
            with self.subTest(status=status), patch.object(release, "urlopen", side_effect=error):
                if status == 404:
                    self.assertFalse(release.version_exists("mixtape-core", VERSION))
                else:
                    with self.assertRaises(RuntimeError):
                        release.version_exists("mixtape-core", VERSION)

    def test_network_failure_never_means_already_published(self):
        for error in [URLError("synthetic"), TimeoutError()]:
            with self.subTest(error=error), patch.object(release, "urlopen", side_effect=error):
                with self.assertRaises(RuntimeError):
                    release.version_exists("mixtape-core", VERSION)

    def test_malformed_mismatched_or_yanked_responses_stop_release(self):
        for data in [
            {}, [],
            {"version": {"crate": "wrong-crate", "num": VERSION, "yanked": False}},
            {"version": {"crate": "mixtape-core", "num": "0.4.0", "yanked": False}},
            {"version": {"crate": "mixtape-core", "num": VERSION, "yanked": True}},
            {"version": {"crate": "mixtape-core", "num": VERSION}},
        ]:
            with self.subTest(data=data), patch.object(release, "urlopen", return_value=io.BytesIO(json.dumps(data).encode())):
                with self.assertRaises(RuntimeError):
                    release.version_exists("mixtape-core", VERSION)
        with patch.object(release, "urlopen", return_value=io.BytesIO(b"not JSON")):
            with self.assertRaises(RuntimeError):
                release.version_exists("mixtape-core", VERSION)


class PublishTests(unittest.TestCase):
    def setUp(self):
        self.workspace = self.enterContext(patch.object(release, "validate_workspace"))
        self.exists = self.enterContext(patch.object(release, "version_exists"))
        self.run = self.enterContext(patch.object(release.subprocess, "run"))
        self.sleep = self.enterContext(patch.object(release.time, "sleep"))
        self.enterContext(patch("sys.stdout", new_callable=io.StringIO))

    def test_publishes_every_crate_in_dependency_order_and_confirms_result(self):
        self.exists.side_effect = [False, True] * len(release.CRATES)
        release.publish(VERSION)
        self.workspace.assert_called_once_with(VERSION)
        self.assertEqual(self.run.call_args_list, [
            call(["cargo", "publish", "--locked", "--registry", "crates-io", "-p", crate], cwd=release.ROOT, check=True)
            for crate in release.CRATES
        ])

    def test_resume_skips_only_confirmed_existing_versions(self):
        self.exists.side_effect = [True, False, True] + [True] * 4
        release.publish(VERSION)
        self.run.assert_called_once_with(
            ["cargo", "publish", "--locked", "--registry", "crates-io", "-p", "mixtape-core"],
            cwd=release.ROOT, check=True,
        )

    def test_cargo_failure_stops_before_next_crate(self):
        self.exists.return_value = False
        self.run.side_effect = subprocess.CalledProcessError(101, ["cargo", "publish"])
        with self.assertRaises(subprocess.CalledProcessError):
            release.publish(VERSION)
        self.assertEqual(self.run.call_count, 1)
        self.assertEqual(self.exists.call_count, 1)
        self.sleep.assert_not_called()

    def test_registry_failure_does_not_publish(self):
        self.exists.side_effect = RuntimeError("registry unavailable")
        with self.assertRaises(RuntimeError):
            release.publish(VERSION)
        self.run.assert_not_called()

    def test_invisible_upload_stops_before_next_crate(self):
        self.exists.return_value = False
        with self.assertRaisesRegex(RuntimeError, "not visible"):
            release.publish(VERSION)
        self.assertEqual(self.run.call_count, 1)

    def test_invalid_workspace_stops_before_network_access(self):
        self.workspace.side_effect = RuntimeError("wrong tag")
        with self.assertRaises(RuntimeError):
            release.publish(VERSION)
        self.exists.assert_not_called()
        self.run.assert_not_called()


class WorkflowTests(unittest.TestCase):
    def test_release_is_announced_only_after_successful_publication(self):
        workflow = (Path(__file__).resolve().parent.parent / ".github/workflows/release.yml").read_text()
        self.assertLess(workflow.index("Publish crates to crates.io"), workflow.index("Create or update GitHub Release"))
        self.assertIn('python3 scripts/publish_crates.py "$RELEASE_VERSION"', workflow)
        self.assertIn('--notes-file "$RUNNER_TEMP/mixtape-release-notes.txt"', workflow)
        self.assertNotIn("cargo publish -p", workflow)


if __name__ == "__main__":
    unittest.main()
