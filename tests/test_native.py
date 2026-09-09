import contextlib
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import native


class NativeBuildTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.archive = self.root / 'source.zip'
        with zipfile.ZipFile(self.archive, 'w') as archive:
            entry = zipfile.ZipInfo('build.sh')
            entry.external_attr = 0o100755 << 16
            archive.writestr(entry, '#!/bin/sh\nprintf built >> count\ncp payload app\nchmod 755 app\n')
            archive.writestr('payload', '#!/bin/sh\nexit 0\n')
        self.request = {'projectKey':'test-project','sourceHash':native.sha(self.archive),
                        'archive':str(self.archive),'framework':'custom',
                        'command':'/bin/sh build.sh','artifact':'app'}
        self.tools = SimpleNamespace(config={}, os='linux', profile={'target':'aarch64-unknown-linux-gnu'},
                                     env=os.environ.copy(), doctor=lambda: {'ready': True, 'tools':{}, 'missing': []})
        self.workspace = patch.object(native, 'local_workspace', return_value=self.root / 'workspace')
        self.workspace.start()
        self.addCleanup(self.workspace.stop)

    def build(self):
        with contextlib.redirect_stdout(io.StringIO()):
            return native.build(self.request, self.tools)

    def test_cache_reuses_verified_output_and_rebuilds_changed_output(self):
        first = self.build()
        executable = Path(first['executable'])
        count = executable.parent / 'count'
        self.build()
        self.assertEqual(count.read_text(), 'built')
        executable.write_text('changed output')
        rebuilt = self.build()
        self.assertEqual(count.read_text(), 'builtbuilt')
        self.assertEqual(first['executableSHA256'], rebuilt['executableSHA256'])

    def test_archive_checksum_must_match_before_running_any_build(self):
        self.archive.write_bytes(b'changed input')
        with self.assertRaisesRegex(RuntimeError, 'checksum mismatch'):
            self.build()
        self.assertFalse((self.root / 'workspace' / 'test-project').exists())

    def test_archive_paths_cannot_escape_the_project(self):
        with zipfile.ZipFile(self.archive, 'w') as archive:
            archive.writestr('../../outside', 'must not extract')
        self.request['sourceHash'] = native.sha(self.archive)
        with self.assertRaises(ValueError):
            self.build()
        self.assertFalse((self.root / 'outside').exists())

    def test_executable_source_scripts_keep_their_mode(self):
        self.request['command'] = './build.sh'
        receipt = self.build()
        self.assertTrue(Path(receipt['executable']).is_file())

    def test_ci_runner_executes_run_steps_and_records_explicit_limits(self):
        self.request.update({
            'workflow': {'name': 'fixture', 'event': 'workflow_dispatch', 'jobs': []},
            'stages': {
                'setup': [{'index': 1, 'name': 'checkout', 'adapter': 'checkout'}],
                'test': [{'index': 2, 'name': 'skip test', 'adapter': 'skip', 'reason': 'fixture'}],
                'build': [{'index': 3, 'name': 'build', 'adapter': 'run', 'run': 'printf ci-ok > ci.txt'}],
                'smoke': [{'index': 4, 'name': 'skip smoke', 'adapter': 'skip', 'reason': 'fixture'}],
            },
            'revision': 'fixture', 'dirty': True, 'target': 'aarch64-unknown-linux-gnu',
        })
        self.tools.setup_system = lambda: None
        self.tools.setup_user = lambda: None
        with contextlib.redirect_stdout(io.StringIO()):
            report = native.ci_run(self.request, self.tools)
        self.assertTrue(report['success'])
        self.assertEqual(report['status'], 'passed_with_limits')
        self.assertIn('build', report['stages'])
        self.assertTrue(any('test skipped' in value for value in report['limits']))


class ProcessLookupTests(unittest.TestCase):
    def test_process_is_matched_by_full_executable_path(self):
        with tempfile.TemporaryDirectory(prefix='build process ') as directory:
            executable = Path(directory) / 'sleep'
            subprocess.run(['cc', '-x', 'c', '-o', str(executable), '-'],
                           input='#include <unistd.h>\nint main(void) { sleep(30); return 0; }\n',
                           text=True, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            process = subprocess.Popen([str(executable)])
            try:
                for _ in range(20):
                    found = native.running_pid(executable)
                    if found:
                        break
                    time.sleep(0.05)
                detail = subprocess.check_output(['ps', '-p', str(process.pid), '-o', 'comm='], text=True)
                self.assertEqual(found, process.pid, detail)
                self.assertIsNone(native.running_pid(Path(directory) / 'other' / 'sleep'))
            finally:
                process.terminate()
                process.wait()


if __name__ == '__main__':
    unittest.main()
