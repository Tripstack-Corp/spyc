"""`scripts/jar_fixtures.py` picks every stable release from 3.0.0 on, and
the tarball that holds this machine's `spyc-jar`."""

import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "jar_fixtures", Path(__file__).resolve().parent / "jar_fixtures.py")
jar_fixtures = importlib.util.module_from_spec(spec)
spec.loader.exec_module(jar_fixtures)


def release(tag, pre=False, draft=False):
    return {"tagName": tag, "isPrerelease": pre, "isDraft": draft}


class JarFixtures(unittest.TestCase):
    def test_every_stable_release_from_3_0_0_on(self):
        releases = [release("v3.1.0"), release("v2.4.0"), release("v3.0.0"),
                    release("v3.0.0-rc.2", pre=True), release("v3.2.0", draft=True),
                    release("v10.0.0"), release("nightly", pre=True)]
        self.assertEqual(jar_fixtures.fixture_tags(releases), ["v3.0.0", "v3.1.0", "v10.0.0"])

    def test_none_before_the_first_jar(self):
        self.assertEqual(jar_fixtures.fixture_tags([release("v2.4.0"), release("v2.3.1")]), [])

    def test_the_tarball_for_each_platform(self):
        self.assertEqual(jar_fixtures.asset("v3.0.0", "Darwin", "arm64"),
                         "spyc-v3.0.0-macos-universal.tar.gz")
        self.assertEqual(jar_fixtures.asset("v3.0.0", "Linux", "x86_64"),
                         "spyc-v3.0.0-linux-x86_64.tar.gz")
        self.assertEqual(jar_fixtures.asset("v3.0.0", "Linux", "aarch64"),
                         "spyc-v3.0.0-linux-aarch64.tar.gz")
        with self.assertRaises(SystemExit):
            jar_fixtures.asset("v3.0.0", "FreeBSD", "amd64")

    def test_the_jar_is_handed_over_by_the_name_the_test_reads(self):
        test = (Path(__file__).resolve().parent.parent / "tests/jar_compat.rs").read_text(encoding="utf-8")
        self.assertIn(f'const OLD_BIN: &str = "{jar_fixtures.ENV}";', test)
        self.assertIn("std::env::var_os(OLD_BIN)", test)

    def test_the_names_match_what_release_yml_packs(self):
        text = (Path(__file__).resolve().parent.parent / ".github/workflows/release.yml").read_text(
            encoding="utf-8")
        for name in ("macos-universal", "linux-x86_64", "linux-aarch64"):
            self.assertIn(f'"spyc-${{GITHUB_REF_NAME}}-{name}.tar.gz" spyc spyc-jar', text)


if __name__ == "__main__":
    unittest.main()
