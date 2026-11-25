"""spyc-jar carries spyc's version, in lockstep (the 3.0 review's F2).

`cargo install spyc` builds spyc's `spyc-jar` binary from the `spyc-jar`
crate that crates.io holds at the version spyc's manifest asks for. A jar
crate versioned apart from spyc was published once and skipped by every
later release, so `cargo install` built later releases' jars from the code
of the one that first published it. In lockstep each tag publishes both, an
rc's both as prereleases, and spyc asks for exactly its own.
"""

from pathlib import Path
import os
import re
import shutil
import subprocess
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parent.parent
SPYC = Path("Cargo.toml")
JAR = Path("crates/spyc-jar/Cargo.toml")


def manifest(root, path):
    return tomllib.loads((root / path).read_text(encoding="utf-8"))


def locked(name):
    """The version Cargo.lock holds for the workspace member `name`."""
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    versions = [p["version"] for p in lock["package"] if p["name"] == name and "source" not in p]
    if len(versions) != 1:
        raise AssertionError(f"Cargo.lock has {len(versions)} entries for the member {name}")
    return versions[0]


class SpycJarTakesSpycsVersion(unittest.TestCase):
    @unittest.skip("red until F2 (fix/jar-lockstep) puts spyc-jar in lockstep with spyc")
    def test_spyc_jar_carries_spycs_version(self):
        spyc = manifest(ROOT, SPYC)["package"]["version"]
        jar = manifest(ROOT, JAR)["package"]["version"]
        self.assertTrue(spyc, "premise: Cargo.toml has a version")
        self.assertEqual(jar, spyc, f"{JAR} is {jar} while spyc is {spyc}")
        self.assertEqual(locked("spyc-jar"), locked("spyc"), "Cargo.lock")

    @unittest.skip("red until F2 (fix/jar-lockstep) puts spyc-jar in lockstep with spyc")
    def test_spyc_asks_for_exactly_its_own_jar(self):
        spyc = manifest(ROOT, SPYC)["package"]["version"]
        dep = manifest(ROOT, SPYC)["dependencies"]["spyc-jar"]
        self.assertEqual(dep.get("version"), f"={spyc}",
                         "a caret requirement lets crates.io hand `cargo install` another jar")
        self.assertEqual(dep.get("path"), "crates/spyc-jar")

    @unittest.skip("red until F2 (fix/jar-lockstep) puts spyc-jar in lockstep with spyc")
    def test_set_version_moves_both(self):
        with tempfile.TemporaryDirectory() as d:
            for path in (SPYC, JAR):
                (Path(d) / path).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy(ROOT / path, Path(d) / path)
            r = subprocess.run(["make", "--no-print-directory", "-f", str(ROOT / "Makefile"),
                                "set-version", "VERSION=9.9.9-rc.1"],
                               cwd=d, capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
            self.assertEqual(manifest(Path(d), SPYC)["package"]["version"], "9.9.9-rc.1")
            self.assertEqual(manifest(Path(d), JAR)["package"]["version"], "9.9.9-rc.1")
            self.assertEqual(manifest(Path(d), SPYC)["dependencies"]["spyc-jar"]["version"],
                             "=9.9.9-rc.1")
            for path in (SPYC, JAR):
                before = (ROOT / path).read_text(encoding="utf-8").splitlines()
                after = (Path(d) / path).read_text(encoding="utf-8").splitlines()
                changed = [a for a, b in zip(before, after) if a != b]
                self.assertEqual(len(before), len(after), path)
                self.assertLessEqual(len(changed), 2, f"{path}: only version lines move: {changed}")

    @unittest.skip("red until F2 (fix/jar-lockstep) puts spyc-jar in lockstep with spyc")
    def test_release_prep_sets_the_version_with_set_version(self):
        text = (ROOT / "Makefile").read_text(encoding="utf-8")
        recipe = text.split("\nrelease-prep:", 1)[1].split("\n\n", 1)[0]
        self.assertIn("set-version VERSION=$(VERSION)", recipe)
        self.assertIn("cargo update -p $(BINARY) -p $(JAR)", recipe)
        self.assertIn(str(JAR), recipe, "release-prep commits the jar's manifest")


def publish_step():
    """release.yml's crates job's `Publish` run script."""
    text = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    m = re.search(r"^      - name: Publish\n        env:\n(?:          .*\n)+?        run: \|\n((?:          .*\n|\n)+)",
                  text, re.M)
    if not m:
        raise AssertionError("release.yml has no crates Publish step to run")
    return "\n".join(line[10:] for line in m.group(1).splitlines())


def tag_repo(d, spyc, jar):
    """A repository at `spyc` and `jar`, its CHANGELOG ready, on `main` and
    level with its `origin`, with this Makefile: what `make release-tag`
    expects on the merged release commit."""
    repo = Path(d, "repo")
    (repo / JAR.parent).mkdir(parents=True)
    shutil.copy(ROOT / "Makefile", repo / "Makefile")
    (repo / SPYC).write_text(f'[package]\nname = "spyc"\nversion = "{spyc}"\n', encoding="utf-8")
    (repo / JAR).write_text(f'[package]\nname = "spyc-jar"\nversion = "{jar}"\n', encoding="utf-8")
    (repo / "CHANGELOG.md").write_text(f"## [{spyc}]\n", encoding="utf-8")
    env = git_env()
    def git(*args, cwd=repo):
        subprocess.run(["git", *args], cwd=cwd, env=env, check=True, capture_output=True)
    git("init", "-q", "-b", "main")
    git("add", "-A")
    git("commit", "-q", "-m", "release")
    git("clone", "-q", "--bare", str(repo), str(Path(d, "origin.git")), cwd=d)
    git("remote", "add", "origin", str(Path(d, "origin.git")))
    git("fetch", "-q", "origin")
    return repo


def git_env():
    """An environment for a throwaway repository: nothing of the caller's git
    state, no signing."""
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    env.update({"GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.invalid",
                "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.invalid",
                "GIT_CONFIG_COUNT": "2", "GIT_CONFIG_KEY_0": "commit.gpgsign", "GIT_CONFIG_VALUE_0": "false",
                "GIT_CONFIG_KEY_1": "tag.gpgsign", "GIT_CONFIG_VALUE_1": "false"})
    return env


class ReleaseChecksTheJarsVersion(unittest.TestCase):
    """The two release steps after the version is set refuse a tree whose jar
    isn't at the tag's version, should one reach them past the test above."""

    def release_tag(self, d, spyc, jar):
        repo = tag_repo(d, spyc, jar)
        r = subprocess.run(["make", "--no-print-directory", "release-tag", f"VERSION={spyc}"],
                           cwd=repo, env=git_env(), capture_output=True, text=True)
        tagged = subprocess.run(["git", "tag", "-l", f"v{spyc}"], cwd=repo, env=git_env(),
                                capture_output=True, text=True, check=True).stdout.strip()
        return r, tagged

    def test_release_tag_tags_a_tree_in_lockstep(self):
        with tempfile.TemporaryDirectory() as d:
            r, tagged = self.release_tag(d, "9.9.9", "9.9.9")
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
            self.assertEqual(tagged, "v9.9.9")

    @unittest.skip("red until F2 (fix/jar-lockstep) puts spyc-jar in lockstep with spyc")
    def test_release_tag_refuses_a_jar_at_another_version(self):
        with tempfile.TemporaryDirectory() as d:
            r, tagged = self.release_tag(d, "9.9.9", "9.9.8")
            self.assertNotEqual(r.returncode, 0, r.stdout + r.stderr)
            self.assertIn(str(JAR), r.stdout + r.stderr, "the refusal names the jar's manifest")
            self.assertEqual(tagged, "", "no tag")

    def publish(self, d, tag, spyc, jar):
        """Run the Publish step for `tag` with stand-ins: `cargo metadata`
        reports the versions, crates.io has none of them, and `cargo publish`
        writes down what it would publish."""
        bin_dir = Path(d, "bin")
        bin_dir.mkdir()
        published = Path(d, "published")
        published.touch()
        cargo = bin_dir / "cargo"
        cargo.write_text(
            "#!/bin/sh\n"
            'case "$1" in\n'
            f'  metadata) echo \'{{"packages":[{{"name":"spyc","version":"{spyc}"}},'
            f'{{"name":"spyc-jar","version":"{jar}"}},{{"name":"spyc-vt-sys","version":"0.2.0"}}]}}\' ;;\n'
            f'  publish) echo "$4" >> "{published}" ;;\n'
            "  *) exit 2 ;;\n"
            "esac\n", encoding="utf-8")
        curl = bin_dir / "curl"
        curl.write_text("#!/bin/sh\nexit 22\n", encoding="utf-8")
        for f in (cargo, curl):
            f.chmod(0o755)
        r = subprocess.run(["bash", "-c", publish_step()], cwd=d, capture_output=True, text=True,
                           env={"GITHUB_REF_NAME": tag, "PATH": f"{bin_dir}:/usr/bin:/bin"})
        return r, published.read_text(encoding="utf-8").split()

    @unittest.skipUnless(shutil.which("jq"), "needs jq")
    def test_the_crates_job_publishes_both_at_the_tags_version(self):
        with tempfile.TemporaryDirectory() as d:
            r, published = self.publish(d, "v9.9.9-rc.1", "9.9.9-rc.1", "9.9.9-rc.1")
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
            self.assertEqual(published, ["spyc-vt-sys", "spyc-jar", "spyc"], "in dependency order")

    @unittest.skip("red until F2 (fix/jar-lockstep) puts spyc-jar in lockstep with spyc")
    @unittest.skipUnless(shutil.which("jq"), "needs jq")
    def test_the_crates_job_refuses_a_jar_at_another_version(self):
        with tempfile.TemporaryDirectory() as d:
            r, published = self.publish(d, "v9.9.9", "9.9.9", "0.1.0")
            self.assertNotEqual(r.returncode, 0, r.stdout + r.stderr)
            self.assertEqual(published, [], "nothing published")


if __name__ == "__main__":
    unittest.main()
