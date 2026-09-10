import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import build


class RetentionTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.state = Path(directory.name) / '.state'
        (self.state / 'runs').mkdir(parents=True)
        (self.state / 'logs').mkdir(parents=True)
        self.patcher = patch.object(build, 'STATE', self.state)
        self.patcher.start()
        self.addCleanup(self.patcher.stop)

    def run_dir(self, name, age_seconds=0, project='/app', status='success', payload=b'', platform='linux'):
        """Create a retained run with the report and log layout the controller writes."""
        directory = self.state / 'runs' / name
        directory.mkdir(parents=True, exist_ok=True)
        report = directory / 'report.json'
        report.write_text(json.dumps({'runId': name, 'project': project, 'status': status}) + '\n')
        log = self.state / 'logs' / ('%s-%s.log' % (name, platform))
        log.write_bytes(payload)
        when = time.time() - age_seconds
        for path in (report, log, directory):
            os.utime(path, (when, when))
        return directory

    def names(self):
        return sorted(path.name for path in (self.state / 'runs').iterdir())

    def test_byte_cap_removes_the_oldest_run_not_the_newest(self):
        for index, name in enumerate(['old', 'mid', 'new']):
            self.run_dir(name, age_seconds=3000 - index * 1000, payload=b'x' * 5000)
        self.run_dir('current', payload=b'x' * 10)
        build.retain_run({'project': '/app'}, 'current',
                         {'retention': {'maxRunsPerProject': 20, 'maxBytes': 12000, 'days': 3650}})
        self.assertEqual(self.names(), ['current', 'mid', 'new'])

    def test_evicting_a_run_also_removes_its_logs(self):
        self.run_dir('old', age_seconds=3000, payload=b'x' * 5000)
        self.run_dir('current')
        build.retain_run({'project': '/app'}, 'current',
                         {'retention': {'maxRunsPerProject': 20, 'maxBytes': 100, 'days': 3650}})
        self.assertEqual(self.names(), ['current'])
        self.assertFalse(list((self.state / 'logs').glob('old-*.log')))

    def test_per_project_limit_keeps_the_newest_runs_of_each_project(self):
        for index in range(4):
            self.run_dir('a%s' % index, age_seconds=4000 - index * 100, project='/first')
        self.run_dir('b0', age_seconds=4000, project='/second')
        build.retain_run({'project': '/first'}, 'current',
                         {'retention': {'maxRunsPerProject': 2, 'maxBytes': 10 ** 12, 'days': 3650}})
        # The current run occupies one of /first's two slots, so one older run survives.
        self.assertEqual(self.names(), ['a3', 'b0'])

    def test_expiry_reclaims_an_abandoned_running_report(self):
        """A killed build stays `running` forever and would otherwise mask the
        project's last real result in the dashboard permanently."""
        self.run_dir('stale', age_seconds=60 * 60 * 24 * 40, status='running')
        self.run_dir('recent', age_seconds=60, status='running')
        build.retain_run({'project': '/app'}, 'current', {'retention': {'days': 30}})
        self.assertEqual(self.names(), ['recent'])

    def test_a_running_report_survives_the_count_and_byte_policies(self):
        self.run_dir('busy', age_seconds=100, status='running', payload=b'x' * 5000)
        build.retain_run({'project': '/app'}, 'current',
                         {'retention': {'maxRunsPerProject': 1, 'maxBytes': 10, 'days': 3650}})
        self.assertEqual(self.names(), ['busy'])

    def test_the_current_run_is_never_removed(self):
        self.run_dir('current', payload=b'x' * 9000)
        build.retain_run({'project': '/app'}, 'current',
                         {'retention': {'maxRunsPerProject': 1, 'maxBytes': 1, 'days': 1}})
        self.assertEqual(self.names(), ['current'])

    def test_the_current_result_file_is_not_pruned_before_the_caller_reads_it(self):
        gui = self.state / 'gui'
        gui.mkdir()
        current = gui / 'current.json'
        stale = gui / 'stale.json'
        for path in (current, stale):
            path.write_text('{}')
            when = time.time() - 60 * 60 * 24 * 40
            os.utime(path, (when, when))
        build.retain_run({'project': '/app'}, 'current', {'retention': {'days': 30}}, keep=(current,))
        self.assertTrue(current.is_file())
        self.assertFalse(stale.exists())

    def test_legacy_reports_are_migrated_with_their_recorded_time(self):
        legacy = self.state / '20260101-120000-000000-result.json'
        legacy.write_text(json.dumps({'action': 'build', 'project': '/app', 'status': 'success'}) + '\n')
        when = time.time() - 60 * 60 * 24
        os.utime(legacy, (when, when))
        self.assertEqual(build.migrate_legacy_runs(), 1)
        moved = self.state / 'runs' / '20260101-120000-000000' / 'report.json'
        self.assertFalse(legacy.exists())
        self.assertEqual(json.loads(moved.read_text())['project'], '/app')
        self.assertAlmostEqual(moved.stat().st_mtime, when, delta=1)

    def test_migration_does_not_replace_an_existing_report_or_escape_the_run_directory(self):
        existing = self.state / 'runs' / 'kept'
        existing.mkdir()
        (existing / 'report.json').write_text(json.dumps({'project': '/newer'}) + '\n')
        (self.state / 'kept-result.json').write_text(json.dumps({'project': '/older'}) + '\n')
        (self.state / '..-result.json').write_text('{}')
        build.migrate_legacy_runs()
        self.assertEqual(json.loads((existing / 'report.json').read_text())['project'], '/newer')
        self.assertFalse((self.state / 'kept-result.json').exists())
        self.assertFalse((self.state / 'report.json').exists())

    def test_migration_ignores_unrelated_state_files(self):
        (self.state / 'tool-status.json').write_text('{}')
        (self.state / 'gui-dashboard.json').write_text('{}')
        self.assertEqual(build.migrate_legacy_runs(), 0)
        self.assertTrue((self.state / 'tool-status.json').is_file())
        self.assertTrue((self.state / 'gui-dashboard.json').is_file())

    def test_expired_logs_with_no_run_are_removed_but_retained_runs_keep_theirs(self):
        """winbuild.py keeps no run report, so only age can bound its logs."""
        self.run_dir('kept', age_seconds=60)
        old = self.state / 'logs' / '20250101-090000-000000.log'
        old.write_text('legacy entry point output')
        when = time.time() - 60 * 60 * 24 * 40
        os.utime(old, (when, when))
        recent = self.state / 'logs' / '20260101-090000-000000.log'
        recent.write_text('recent legacy output')
        build.retain_run({'project': '/app'}, 'current', {'retention': {'days': 30}})
        self.assertFalse(old.exists())
        self.assertTrue(recent.is_file())
        self.assertTrue((self.state / 'logs' / 'kept-linux.log').is_file())


if __name__ == '__main__':
    unittest.main()
