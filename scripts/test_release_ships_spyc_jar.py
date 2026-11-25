"""Every release package carries spyc-jar beside spyc: spyc runs each pane
tab's program in one, and with none beside it offers to run tabs without.

Runs release.yml's own packaging steps on stand-in binaries and lists each
tarball, and `make deb`'s recipe on stand-ins and lists each deb (where
`dpkg-deb` is installed: CI's Linux runners).
"""

from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
RELEASE = ROOT / ".github/workflows/release.yml"


def packaging_steps():
    """release.yml's `Package tarball(s)` run scripts."""
    text = RELEASE.read_text(encoding="utf-8")
    scripts = []
    for m in re.finditer(r"^      - name: Package tarballs?\n        run: \|\n((?:          .*\n|\n)+)", text, re.M):
        scripts.append("\n".join(line[10:] for line in m.group(1).splitlines()))
    return scripts


class ReleaseShipsSpycJar(unittest.TestCase):
    def test_each_release_tarball_holds_spyc_and_spyc_jar(self):
        scripts = packaging_steps()
        self.assertEqual(len(scripts), 2, "release.yml's macOS and Linux packaging steps")
        tarballs = 0
        for script in scripts:
            with tempfile.TemporaryDirectory() as d:
                # Every binary the step installs from, as a stand-in.
                for src in re.findall(r"install -m 0755 (\S+) \S+", script):
                    Path(d, src).parent.mkdir(parents=True, exist_ok=True)
                    Path(d, src).write_text("#!/bin/sh\n", encoding="utf-8")
                for name in ("LICENSE", "README.md"):
                    Path(d, name).write_text(name, encoding="utf-8")
                r = subprocess.run(["bash", "-euo", "pipefail", "-c", script], cwd=d,
                                   env={"GITHUB_REF_NAME": "v9.9.9", "PATH": "/usr/bin:/bin"},
                                   capture_output=True, text=True)
                self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
                for tarball in Path(d).glob("spyc-v9.9.9-*.tar.gz"):
                    tarballs += 1
                    with tarfile.open(tarball) as t:
                        names = set(t.getnames())
                    self.assertTrue({"spyc", "spyc-jar"} <= names, f"{tarball.name}: {sorted(names)}")
        self.assertEqual(tarballs, 3, "macOS universal and both Linux tarballs")

    @unittest.skipUnless(shutil.which("dpkg-deb"), "needs dpkg-deb")
    def test_the_deb_installs_spyc_jar_beside_spyc(self):
        with tempfile.TemporaryDirectory() as d:
            built = Path(d, "built")
            built.mkdir()
            for name in ("spyc", "spyc-jar"):
                Path(built, name).write_text("#!/bin/sh\n", encoding="utf-8")
            r = subprocess.run(["make", "--no-print-directory", "_deb", "ARCH=amd64",
                                f"SRC={built / 'spyc'}", "VERSION=9.9.9", f"DIST_DIR={d}/dist"],
                               cwd=ROOT, capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
            listing = subprocess.run(["dpkg-deb", "-c", f"{d}/dist/spyc_9.9.9_amd64.deb"],
                                     capture_output=True, text=True, check=True).stdout
            self.assertIn("./usr/bin/spyc\n", listing)
            self.assertIn("./usr/bin/spyc-jar\n", listing)

    @unittest.skip("red until F8 (fix/deb-needs-jar) requires spyc-jar in a 3.x deb")
    def test_a_deb_of_a_release_that_ships_spyc_jar_needs_it(self):
        """From 3.0.0 every release ships spyc-jar, and spyc without it runs
        no tab: a deb built without it fails rather than ship (the 3.0
        review's F8)."""
        for version in ("3.0.0-rc.1", "3.0.0", "3.1.2"):
            with self.subTest(version=version), tempfile.TemporaryDirectory() as d:
                built = Path(d, "built")
                built.mkdir()
                Path(built, "spyc").write_text("#!/bin/sh\n", encoding="utf-8")
                r = subprocess.run(["make", "--no-print-directory", "_deb", "ARCH=amd64",
                                    f"SRC={built / 'spyc'}", f"VERSION={version}", f"DIST_DIR={d}/dist"],
                                   cwd=ROOT, capture_output=True, text=True)
                self.assertNotEqual(r.returncode, 0, r.stdout + r.stderr)
                self.assertIn("spyc-jar", r.stdout + r.stderr, "the failure names what's missing")
                self.assertEqual(list(Path(d).glob("dist/*.deb")), [])

    @unittest.skipUnless(shutil.which("dpkg-deb"), "needs dpkg-deb")
    def test_a_release_from_before_the_jar_still_packages_without_it(self):
        """apt.yml rebuilds every release's deb, those from before spyc-jar
        shipped among them."""
        with tempfile.TemporaryDirectory() as d:
            built = Path(d, "built")
            built.mkdir()
            Path(built, "spyc").write_text("#!/bin/sh\n", encoding="utf-8")
            r = subprocess.run(["make", "--no-print-directory", "_deb", "ARCH=amd64",
                                f"SRC={built / 'spyc'}", "VERSION=2.4.0", f"DIST_DIR={d}/dist"],
                               cwd=ROOT, capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
            listing = subprocess.run(["dpkg-deb", "-c", f"{d}/dist/spyc_2.4.0_amd64.deb"],
                                     capture_output=True, text=True, check=True).stdout
            self.assertIn("./usr/bin/spyc\n", listing)
            self.assertNotIn("spyc-jar", listing)


if __name__ == "__main__":
    unittest.main()
