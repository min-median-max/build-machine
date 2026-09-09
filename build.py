#!/usr/bin/env python3
"""Diagnose, provision and build native projects from this Mac."""
import argparse
import datetime
import fcntl
import json
from pathlib import Path
import shlex
import subprocess
import sys

from winbuild import ROOT, STATE, Machine, decode_output, project_key, snapshot_project


class Runner:
    def __init__(self, config, platform, log):
        self.config = config
        self.platform = platform
        self.log = log
        self.vm = config['platforms'][platform].get('vm')
        self.share = Path('/media/psf') / config['share']

    def call(self, args, capture=False):
        args = [str(arg) for arg in args]
        display = shlex.join(args)
        print('> ' + display, flush=True)
        with self.log.open('ab') as log:
            log.write(('> ' + display + '\n').encode())
            log.flush()
            chunks = []
            with subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as process:
                for line in iter(process.stdout.readline, b''):
                    log.write(line)
                    log.flush()
                    chunks.append(line)
                    if not capture:
                        print(decode_output(line).rstrip(), flush=True)
                code = process.wait()
            log.write(('Exit code: %s\n' % code).encode())
        output = decode_output(b''.join(chunks))
        if code:
            raise RuntimeError('Command failed (%s): %s\n%s\nLog: %s' % (code, display, output[-2500:], self.log))
        return output

    def prepare(self):
        if self.platform != 'linux':
            return
        info = json.loads(self.call(['prlctl','list','-i','--json',self.vm], capture=True))[0]
        if info['State'] != 'running' or info.get('GuestTools', {}).get('state') != 'installed':
            raise RuntimeError('Start the Linux VM, sign in to its desktop and install Parallels Tools.')
        folders = info.get('Host Shared Folders', {})
        existing = folders.get(self.config['share'])
        if existing and Path(existing['path']).resolve() != ROOT:
            raise RuntimeError('The named build-machine share already belongs to another directory.')
        if not existing:
            self.call(['prlctl','set',self.vm,'--shf-host-add',self.config['share'],'--path',ROOT,'--mode','ro'])
        elif existing.get('mode') != 'ro' or not existing.get('enabled'):
            self.call(['prlctl','set',self.vm,'--shf-host-set',self.config['share'],'--mode','ro','--enable'])
        if not folders.get('enabled'):
            self.call(['prlctl','set',self.vm,'--shf-host','on'])

    def worker(self, action, request=None, run=False, root=False):
        if self.platform == 'linux':
            args = ['prlctl','exec',self.vm]
            if not root:
                args.append('--current-user')
            args += ['/usr/bin/python3',self.share / 'native.py']
        else:
            args = [sys.executable, ROOT / 'native.py']
        args += [action]
        if request:
            if self.platform == 'linux':
                request = self.share / request.relative_to(ROOT)
            args += ['--request',request]
        if run:
            args += ['--run']
        return self.call(args)

    def execute(self, args, snapshot):
        self.prepare()
        if args.action in ('setup','build','release') and self.platform == 'linux':
            self.worker('setup-system', root=True)
        request = None
        if snapshot:
            data = dict(snapshot)
            if data.get('archive') and self.platform == 'linux':
                data['archive'] = str(self.share / Path(data['archive']).relative_to(ROOT))
            request = STATE / 'projects' / data['projectKey'] / (self.platform + '-request.json')
            request.parent.mkdir(parents=True, exist_ok=True)
            request.write_text(json.dumps(data, indent=2) + '\n')
        self.worker(args.action, request=request, run=getattr(args, 'run', False))


def record_tool_status(config, action, platform, result, log):
    if action not in ('doctor', 'setup'):
        return
    path = STATE / 'tool-status.json'
    status = json.loads(path.read_text()) if path.exists() else {}
    if status.get('configuration') != config:
        status = {'configuration': config, 'results': {}}
    status['results'][platform] = dict(result, action=action, log=str(log))
    partial = path.with_suffix('.partial')
    partial.write_text(json.dumps(status, indent=2) + '\n')
    partial.replace(path)


def write_report(report, path, result_file):
    for destination in dict.fromkeys([path] + ([result_file] if result_file else [])):
        destination.parent.mkdir(parents=True, exist_ok=True)
        partial = destination.with_name(destination.name + '.partial')
        partial.write_text(json.dumps(report, indent=2) + '\n')
        partial.replace(destination)


def execute_operation(args, platforms, stamp, log):
    report_path = STATE / (stamp + '-result.json')
    report = {'action': args.action, 'project': str(args.project.expanduser().resolve()) if hasattr(args, 'project') else None,
              'platforms': platforms, 'status': 'running',
              'startedAt': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'source': None, 'results': {}, 'log': str(log)}
    write_report(report, report_path, args.result_file)
    try:
        config = json.loads((ROOT / 'machine.json').read_text())
        snapshot = snapshot_project(args) if args.action in ('build', 'release') else None
        if args.action == 'run':
            snapshot = {'projectKey': project_key(args.project.expanduser().resolve())}
        report['source'] = snapshot
        for platform in platforms:
            print('PLATFORM: ' + platform, flush=True)
            try:
                if platform == 'windows':
                    machine = Machine(config)
                    machine.log_path = log
                    machine.prepare()
                    if args.action == 'doctor':
                        machine.script('Doctor.ps1')
                    elif args.action == 'run':
                        machine.run(args.project)
                    else:
                        machine.setup()
                        if args.action == 'build':
                            machine.build(args, snapshot)
                        elif args.action == 'release':
                            raise RuntimeError('Windows installer rehearsal is not implemented yet. Use build --run for executable validation.')
                else:
                    Runner(config, platform, log).execute(args, snapshot)
                result = {'success': True}
            except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
                result = {'success': False, 'error': str(error)}
                print('ERROR: ' + str(error), file=sys.stderr, flush=True)
            result['finishedAt'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
            report['results'][platform] = result
            record_tool_status(config, args.action, platform, result, log)
            write_report(report, report_path, args.result_file)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        report['error'] = str(error)
        with log.open('a') as output:
            output.write('ERROR: ' + str(error) + '\n')
        print('ERROR: ' + str(error), file=sys.stderr, flush=True)
    success = not report.get('error') and len(report['results']) == len(platforms) and all(result['success'] for result in report['results'].values())
    report['status'] = 'success' if success else 'failure'
    report['finishedAt'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    write_report(report, report_path, args.result_file)
    print(json.dumps(report, indent=2), flush=True)
    return 0 if success else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='action', required=True)
    for action in ('doctor','setup','build','run','release'):
        command = commands.add_parser(action)
        command.add_argument('--os', nargs='+', choices=['windows','linux','macos','all'], default=['all'])
        command.add_argument('--result-file', type=Path, help='Write the structured operation result to this file')
        if action in ('build','run','release'):
            command.add_argument('project', type=Path)
        if action in ('build','release'):
            command.add_argument('--framework', choices=['auto','tauri','wails2','custom'], default='auto')
            command.add_argument('--command', help='Explicit command in the selected platform shell')
            command.add_argument('--artifact', help='Executable path relative to the source snapshot')
            command.add_argument('--run', action='store_true')
    args = parser.parse_args()
    if sys.platform != 'darwin':
        parser.error('Run this controller on macOS; native.py and windows/*.ps1 are the native workers.')
    if 'all' in args.os and len(args.os) != 1:
        parser.error('Use --os all alone, or list the individual platforms.')
    platforms = ['windows','linux','macos'] if args.os == ['all'] else list(dict.fromkeys(args.os))
    STATE.mkdir(exist_ok=True)
    try:
        with (STATE / 'machine.lock').open('w') as lock:
            try:
                fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise RuntimeError('Another build-machine command is running. Wait for its result.')
            stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
            log = STATE / 'logs' / (stamp + '-matrix.log')
            log.parent.mkdir(parents=True, exist_ok=True)
            print('Host log:', log, flush=True)
            return execute_operation(args, platforms, stamp, log)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print('ERROR:', error, file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
