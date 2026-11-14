"""The Homebrew formula asks for the macOS spyc is built for.

`.cargo/config.toml` sets `MACOSX_DEPLOYMENT_TARGET`, and
`.github/workflows/homebrew.yml` writes `depends_on macos: :$MACOS_MIN` into
the tap's formula on each release. These tests keep the two naming the same
macOS, and run the workflow's rewrite step on a formula shaped like the tap's.
"""

from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
WORKFLOW = ROOT / ".github/workflows/homebrew.yml"

# Homebrew's names for macOS versions (`MacOSVersion::SYMBOLS`).
MACOS_NAMES = {"11": "big_sur", "12": "monterey", "13": "ventura", "14": "sonoma",
               "15": "sequoia", "26": "tahoe"}

FORMULA = """\
class Spyc < Formula
  desc "Keyboard-driven, MCP-native terminal file commander"
  version "2.4.0"

  head do
    url "https://example.invalid/spyc.git", branch: "main"
    depends_on "rust" => :build
  end

  on_macos do
    on_arm do
      url "https://example.invalid/v#{version}/spyc-v#{version}-macos-universal.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000001"
    end
    on_intel do
      url "https://example.invalid/v#{version}/spyc-v#{version}-macos-universal.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000001"
    end
  end

  on_linux do
    on_intel do
      url "https://example.invalid/v#{version}/spyc-v#{version}-linux-x86_64.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000002"
    end
    on_arm do
      url "https://example.invalid/v#{version}/spyc-v#{version}-linux-aarch64.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000003"
    end
  end
end
"""


def rewrite_step():
    """The `Rewrite Formula/spyc.rb` step: (its MACOS_MIN, its run script)."""
    text = WORKFLOW.read_text(encoding="utf-8")
    step = text[text.index("- name: Rewrite Formula/spyc.rb"):]
    step = step[:step.index("\n      - name:", 1)]
    macos = re.search(r"^ +MACOS_MIN: (\w+)$", step, re.M).group(1)
    body = step.split("run: |\n", 1)[1]
    script = "\n".join(line[10:] for line in body.splitlines())
    return macos, script


class HomebrewMacosFloor(unittest.TestCase):
    def test_the_formula_asks_for_the_macos_spyc_is_built_for(self):
        # A pattern, not `tomllib`: that needs Python 3.11, and macOS ships 3.9.
        config = (ROOT / ".cargo/config.toml").read_text(encoding="utf-8")
        found = re.search(r'^MACOSX_DEPLOYMENT_TARGET = "([0-9.]+)"$', config, re.M)
        self.assertIsNotNone(found, "no MACOSX_DEPLOYMENT_TARGET line in .cargo/config.toml")
        target = found.group(1)
        major = target.split(".")[0]
        self.assertEqual(rewrite_step()[0], MACOS_NAMES[major],
                         f"MACOSX_DEPLOYMENT_TARGET is {target}: set MACOS_MIN in homebrew.yml to match")

    def test_the_rewrite_puts_one_requirement_in_on_macos_and_is_idempotent(self):
        macos, script = rewrite_step()
        script = script.replace("${{ steps.ver.outputs.version }}", "2.5.0")
        with tempfile.TemporaryDirectory() as d:
            Path(d, "tap/Formula").mkdir(parents=True)
            formula = Path(d, "tap/Formula/spyc.rb")
            formula.write_text(FORMULA, encoding="utf-8")
            Path(d, "SHA256SUMS").write_text(
                "".join(f"{c * 64}  spyc-v2.5.0-{name}.tar.gz\n"
                        for c, name in [("a", "macos-universal"), ("b", "linux-x86_64"),
                                        ("c", "linux-aarch64")]), encoding="utf-8")
            for run in (1, 2):
                r = subprocess.run(["bash", "-c", script], cwd=d, env={"MACOS_MIN": macos, "PATH": "/usr/bin:/bin"},
                                   capture_output=True, text=True)
                self.assertEqual(r.returncode, 0, f"run {run}: {r.stdout}{r.stderr}")
                out = formula.read_text(encoding="utf-8")
                self.assertEqual(out.count("depends_on macos:"), 1, f"run {run}")
                self.assertIn(f"  on_macos do\n    depends_on macos: :{macos}\n    on_arm do\n", out)
                self.assertIn('  version "2.5.0"', out)
            self.assertIn('depends_on "rust" => :build', out)

    def test_the_rewrite_fails_without_an_on_macos_block(self):
        macos, script = rewrite_step()
        script = script.replace("${{ steps.ver.outputs.version }}", "2.5.0")
        with tempfile.TemporaryDirectory() as d:
            Path(d, "tap/Formula").mkdir(parents=True)
            Path(d, "tap/Formula/spyc.rb").write_text(FORMULA.replace("  on_macos do", "  on_mac do"),
                                                      encoding="utf-8")
            Path(d, "SHA256SUMS").write_text(
                "".join(f"{c * 64}  spyc-v2.5.0-{name}.tar.gz\n"
                        for c, name in [("a", "macos-universal"), ("b", "linux-x86_64"),
                                        ("c", "linux-aarch64")]), encoding="utf-8")
            r = subprocess.run(["bash", "-c", script], cwd=d, env={"MACOS_MIN": macos, "PATH": "/usr/bin:/bin"},
                               capture_output=True, text=True)
            self.assertNotEqual(r.returncode, 0)
            self.assertIn("no `on_macos do` block", r.stdout)


if __name__ == "__main__":
    unittest.main()
