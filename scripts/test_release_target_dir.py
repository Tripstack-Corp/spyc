"""The release targets read the binaries from the directory `make` built them
in. Outside CI that is a per-checkout directory under ~/.cache, not `target/`,
and `lipo`, `cp` and the deb recipe reading `target/` found nothing (#1149).

Dry-runs (`make -n`) each target with the build directory set to a path of
the test's own, through both the variable a dev machine uses
(`SPYC_TARGET_DIR`) and the one CI does (`CARGO_TARGET_DIR`), and checks
every built binary it names is under it.
"""

from pathlib import Path
import os
import re
import subprocess
import unittest

ROOT = Path(__file__).resolve().parent.parent
BUILT = "/nonexistent/spyc-build-dir"
# A path to a binary cargo builds for a release target.
BINARY_PATH = re.compile(r"(\S*)/(?:aarch64|x86_64)-(?:apple-darwin|unknown-linux-musl)/release/(spyc(?:-jar)?)\b")


def dry_run(target, ci):
    env = {k: v for k, v in os.environ.items() if k not in ("CI", "SPYC_TARGET_DIR", "CARGO_TARGET_DIR")}
    env.update({"SPYC_TARGET_DIR": BUILT, "CARGO_TARGET_DIR": BUILT})
    if ci:
        env["CI"] = "true"
    r = subprocess.run(["make", "-n", "--no-print-directory", target], cwd=ROOT, env=env,
                       capture_output=True, text=True)
    return r


class ReleaseTargetsReadTheBuildDirectory(unittest.TestCase):
    def test_each_release_target_reads_the_binaries_where_make_built_them(self):
        for ci in (False, True):
            for target in ("release-macos-universal", "dist", "deb-x86", "deb-arm"):
                with self.subTest(target=target, ci=ci):
                    r = dry_run(target, ci)
                    self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
                    found = BINARY_PATH.findall(r.stdout)
                    self.assertTrue(found, f"{target} names no built binary:\n{r.stdout}")
                    dirs = {(re.sub(r'^(?:SRC=|")', "", d), b) for d, b in found}
                    wrong = sorted({f"{d}/…/{b}" for d, b in dirs if d != BUILT})
                    self.assertEqual(wrong, [], f"{target} reads outside {BUILT}")


if __name__ == "__main__":
    unittest.main()
