#!/usr/bin/env python3
"""Exercise initial push, unchanged, bumped, downgraded and multi-commit plans locally."""
import os
import shutil
import importlib.machinery
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

loader=importlib.machinery.SourceFileLoader("release_version",str(Path(__file__).with_name("release-version")))
spec=importlib.util.spec_from_loader(loader.name,loader)
module=importlib.util.module_from_spec(spec)
loader.exec_module(module)

class ReleasePlan(unittest.TestCase):
    def test_version_gate(self):
        self.assertTrue(module.plan(None,"0.1.0")["changed"])
        self.assertFalse(module.plan("0.1.0","0.1.0")["changed"])
        self.assertTrue(module.plan("0.1.0","0.2.0")["changed"])
        with self.assertRaises(ValueError):module.plan("0.2.0","0.1.0")
        with self.assertRaises(ValueError):module.plan("0.1.0","0.1.0-rc.1")
        with self.assertRaises(ValueError):module.plan("0.1.0-rc.10","0.1.0-rc.2")
        self.assertTrue(module.plan("0.1.0-rc.2","0.1.0-rc.10")["changed"])
        self.assertTrue(module.plan("0.1.0-rc.10","0.1.0")["changed"])
    def test_reads_package_not_dependency_version(self):
        self.assertEqual(module.version('[package]\nname="test"\nversion="0.2.1"\n[dependencies]\nserde={version="1"}'),"0.2.1")
    def test_real_git_push_range(self):
        with tempfile.TemporaryDirectory() as folder:
            def git(*args):return subprocess.check_output(["git","-C",folder,*args],text=True).strip()
            git("init","-q");git("config","user.name","Test");git("config","user.email","test@example.invalid")
            Path(folder,"README").touch();git("add",".");git("commit","-qm","Initial")
            before=git("rev-parse","HEAD")
            Path(folder,"Cargo.toml").write_text('[package]\nname="test"\nversion="0.1.0"\n')
            git("add",".");git("commit","-qm","Initialize crate")
            first=git("rev-parse","HEAD")
            Path(folder,"README").write_text("docs");git("add",".");git("commit","-qm","Docs")
            helper=str(Path(__file__).with_name("release-version").resolve())
            plan=subprocess.check_output([helper,before,"HEAD"],cwd=folder,text=True)
            self.assertIn('"changed": true',plan)
            unchanged=subprocess.check_output([helper,first,"HEAD"],cwd=folder,text=True)
            self.assertIn('"changed": false',unchanged)

class PublishSafety(unittest.TestCase):
    def test_mocked_registry_and_publisher(self):
        # Execute copies, never the real Cargo/network clients or a real token.
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / "scripts").mkdir()
            for name in ("release-version", "publish-crate"):
                shutil.copy2(Path(__file__).with_name(name), root / "scripts" / name)
            (root / "Cargo.toml").write_text('[package]\nname="dioxus-map-editor"\nversion="0.1.0"\n')
            binary = root / "bin"
            binary.mkdir()
            (binary / "curl").write_text('''#!/usr/bin/env python3
import os,sys
from pathlib import Path
Path(sys.argv[sys.argv.index("-o")+1]).write_text(os.environ.get("MOCK_INDEX", ""))
print(os.environ["MOCK_HTTP"], end="")
''')
            (binary / "cargo").write_text('''#!/usr/bin/env python3
import os,sys
from pathlib import Path
Path(os.environ["MOCK_LOG"]).write_text(" ".join(sys.argv[1:]))
''')
            for file in binary.iterdir():file.chmod(0o755)
            log = root / "published"
            env = dict(os.environ, PATH=str(binary)+os.pathsep+os.environ["PATH"], MOCK_LOG=str(log), CARGO_REGISTRY_TOKEN="test-only-sentinel")
            cases = [
                ("404", "", True, True),
                ("200", '{"vers":"0.0.1"}\n', True, True),
                ("200", '{"vers":"0.1.0","yanked":true}\n', True, False),
                ("500", "", False, False),
                ("200", 'broken-json', False, False),
                ("200", '{}', False, False),
            ]
            for status, index, success, publishes in cases:
                with self.subTest(status=status, index=index):
                    if log.exists():log.unlink()
                    result = subprocess.run([str(root/"scripts/publish-crate"), "0.1.0"], env=dict(env, MOCK_HTTP=status, MOCK_INDEX=index), capture_output=True, text=True)
                    self.assertEqual(result.returncode == 0, success, result.stderr)
                    self.assertEqual(log.exists(), publishes)
                    self.assertNotIn("test-only-sentinel", result.stdout + result.stderr)
            env.pop("CARGO_REGISTRY_TOKEN")
            if log.exists():log.unlink()
            result = subprocess.run([str(root/"scripts/publish-crate"), "0.1.0"], env=dict(env, MOCK_HTTP="404"), capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(log.exists())

if __name__=="__main__":unittest.main()
