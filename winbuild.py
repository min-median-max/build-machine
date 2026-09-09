#!/usr/bin/env python3
"""Provision and operate the Parallels Windows build machine without an agent."""
import argparse
import base64
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parent
STATE = ROOT / '.state'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def ps_quote(value):
    return "'" + str(value).replace("'", "''") + "'"


def decode_output(raw):
    try:
        return raw.decode('utf-8')
    except UnicodeDecodeError:
        return raw.decode('cp949', errors='replace')


def git(project, *args):
    return subprocess.check_output(['git', '-C', str(project), *args])


def project_key(project):
    name = re.sub(r'[^A-Za-z0-9._-]', '-', project.name).strip('.-') or 'project'
    return name[:60] + '-' + digest(os.fsencode(str(project)))[:10]


def make_source_archive(project, destination):
    """Archive current files, honoring Git ignore rules and deterministic ordering."""
    revision = git(project, 'rev-parse', 'HEAD').decode().strip()
    files = sorted(set(git(project, 'ls-files', '-z', '--cached', '--others', '--exclude-standard').split(b'\0')) - {b''})
    destination.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(destination, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        for raw_name in files:
            name = os.fsdecode(raw_name)
            path = project / name
            if not path.exists() and not path.is_symlink():
                continue  # A tracked file deleted in the current working tree.
            try:
                path.resolve().relative_to(project.resolve())
            except ValueError:
                raise ValueError('Source points outside the project: ' + name)
            if not path.is_file():
                raise ValueError('Source is not a regular file (including unsupported submodules): ' + name)
            entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.external_attr = (path.stat().st_mode & 0xFFFF) << 16
            archive.writestr(entry, path.read_bytes())
    return {'revision': revision, 'dirty': bool(git(project, 'status', '--porcelain')), 'sourceHash': digest(destination.read_bytes()), 'fileCount': len(files)}


def controller_files():
    return [ROOT / 'machine.json'] + sorted((ROOT / 'windows').glob('*.ps1'))


def controller_hash():
    return digest(b''.join(path.relative_to(ROOT).as_posix().encode() + b'\0' + path.read_bytes() + b'\0' for path in controller_files()))


class Machine:
    def __init__(self, config, vm=None):
        self.config = config
        self.vm = vm or config['vm']
        self.prlctl = shutil.which('prlctl') or '/Applications/Parallels Desktop.app/Contents/MacOS/prlctl'
        if not Path(self.prlctl).is_file():
            raise RuntimeError('Parallels prlctl is missing. Install and activate Parallels Desktop with CLI support first.')
        self.control_hash = controller_hash()
        self.windows_control = config['windowsRoot'] + '\\control\\' + self.control_hash[:20]
        self.windows_config = self.windows_control + '\\machine.json'
        self.log_path = STATE / 'logs' / (datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f') + '.log')
        self.log_path.parent.mkdir(parents=True, exist_ok=True)

    def cli(self, *args):
        result = subprocess.run([self.prlctl, *args], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        if result.returncode:
            raise RuntimeError(decode_output(result.stdout).strip())
        return decode_output(result.stdout)

    def guest(self, script, system=False):
        script = "$global:ProgressPreference='SilentlyContinue'; " + script
        encoded = base64.b64encode(script.encode('utf-16-le')).decode('ascii')
        command = [self.prlctl, 'exec', self.vm]
        if not system:
            command.append('--current-user')
        command += ['powershell.exe', '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', encoded]
        with self.log_path.open('ab') as log:
            process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            for line in iter(process.stdout.readline, b''):
                log.write(line)
                log.flush()
                print(decode_output(line).rstrip(), flush=True)
            code = process.wait()
        if code:
            raise RuntimeError('Windows command failed with exit code %s. Log: %s' % (code, self.log_path))

    def prepare(self):
        info = json.loads(self.cli('list', '-i', '--json', self.vm))[0]
        if info['State'] != 'running':
            raise RuntimeError('Start the Windows VM and sign in to its desktop before running a build.')
        if info.get('GuestTools', {}).get('state') != 'installed':
            raise RuntimeError('Install Parallels Tools in this Windows VM before using the build machine.')
        folders = info.get('Host Shared Folders', {})
        share = self.config['share']
        existing = folders.get(share)
        if existing and Path(existing['path']).resolve() != ROOT:
            raise RuntimeError('The Parallels share %s is already assigned to another directory.' % share)
        if not existing:
            self.cli('set', self.vm, '--shf-host-add', share, '--path', str(ROOT), '--mode', 'ro')
        elif existing.get('mode') != 'ro' or not existing.get('enabled'):
            self.cli('set', self.vm, '--shf-host-set', share, '--mode', 'ro', '--enable')
        if not folders.get('enabled'):
            self.cli('set', self.vm, '--shf-host', 'on')
        # Only declared control files are copied. They remain usable from Windows.
        archive = STATE / 'control' / (self.control_hash + '.zip')
        archive.parent.mkdir(parents=True, exist_ok=True)
        if not archive.exists():
            with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED) as output:
                for path in controller_files():
                    output.write(path, path.relative_to(ROOT).as_posix())
        source = '\\\\Mac\\' + share + '\\.state\\control\\' + archive.name
        archive_hash = digest(archive.read_bytes())
        self.guest("$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; "
                   "$source=" + ps_quote(source) + "; $dest=" + ps_quote(self.windows_control) + "; "
                   "if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash -ne " + ps_quote(archive_hash) + ") {throw 'Control archive checksum mismatch.'}; "
                   "if (-not (Test-Path -LiteralPath ($dest+'\\ready'))) {"
                   "if (Test-Path -LiteralPath $dest) {throw 'Incomplete control directory. Inspect it before retrying.'}; "
                   "Expand-Archive -LiteralPath $source -DestinationPath $dest; "
                   "Set-Content -LiteralPath ($dest+'\\ready') -Value " + ps_quote(self.control_hash) + "}; "
                   "Write-Output ('Control scripts: '+$dest)")

    def script(self, name, arguments='', system=False):
        path = self.windows_control + '\\windows\\' + name
        self.guest("$ErrorActionPreference='Stop'; & " + ps_quote(path) + ' -ConfigPath ' + ps_quote(self.windows_config) + ' ' + arguments, system=system)

    def setup(self):
        self.script('Setup-Machine.ps1', system=True)
        self.script('Setup-User.ps1')
        self.script('Doctor.ps1')

    def build(self, args):
        project = args.project.expanduser().resolve()
        key = project_key(project)
        transfer = STATE / 'projects' / key
        transfer.mkdir(parents=True, exist_ok=True)
        temporary = transfer / 'source.pending.zip'
        source = make_source_archive(project, temporary)
        archive = transfer / (source['sourceHash'] + '.zip')
        if archive.exists():
            temporary.unlink()
        else:
            temporary.replace(archive)
        framework = args.framework
        if framework == 'auto':
            if (project / 'src-tauri' / 'tauri.conf.json').is_file():
                framework = 'tauri'
            elif (project / 'wails.json').is_file():
                framework = 'wails2'
            else:
                raise ValueError('Use --framework custom --command COMMAND --artifact RELATIVE.exe for this project.')
        if framework == 'custom' and (not args.command or not args.artifact):
            raise ValueError('Custom builds require --command and --artifact.')
        recipe = {'framework': framework, 'command': args.command, 'artifact': args.artifact}
        build_id = digest(json.dumps({'source': source['sourceHash'], 'controller': self.control_hash, 'recipe': recipe}, sort_keys=True).encode())
        request = dict(source, **recipe, projectKey=key, project=str(project), buildId=build_id, controllerHash=self.control_hash,
                       archive='\\\\Mac\\' + self.config['share'] + '\\' + str(archive.relative_to(ROOT)).replace('/', '\\'))
        request_path = transfer / (build_id + '.json')
        request_path.write_text(json.dumps(request, indent=2) + '\n')
        remote_request = '\\\\Mac\\' + self.config['share'] + '\\' + str(request_path.relative_to(ROOT)).replace('/', '\\')
        self.script('Build.ps1', '-RequestPath ' + ps_quote(remote_request))
        (transfer / 'latest.json').write_text(json.dumps(request, indent=2) + '\n')
        if args.run:
            self.run(project)

    def run(self, project):
        key = project_key(project.expanduser().resolve())
        self.script('Run.ps1', '-ProjectKey ' + ps_quote(key))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--vm', help='Parallels VM name; default is machine.json')
    commands = parser.add_subparsers(dest='action', required=True)
    commands.add_parser('setup', help='Check and install all declared Windows build tools')
    commands.add_parser('doctor', help='Report installed tools and missing requirements')
    build = commands.add_parser('build', help='Automatically provision, build, and optionally launch a project')
    build.add_argument('project', type=Path)
    build.add_argument('--framework', choices=['auto','tauri','wails2','custom'], default='auto')
    build.add_argument('--command', help='Explicit Windows command for a custom build')
    build.add_argument('--artifact', help='Executable path relative to the Windows source directory')
    build.add_argument('--run', action='store_true')
    run = commands.add_parser('run', help='Launch or reuse the last successfully built application')
    run.add_argument('project', type=Path)
    args = parser.parse_args()
    if not shutil.which('git'):
        parser.error('Install the macOS command line developer tools (xcode-select --install) for Git.')
    STATE.mkdir(exist_ok=True)
    try:
        # Serialize guest work and shared transfer state across terminal sessions.
        with (STATE / 'machine.lock').open('w') as lock:
            try:
                fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise RuntimeError('Another build-machine command is running. Wait for its logged result.')
            machine = Machine(json.loads((ROOT / 'machine.json').read_text()), args.vm)
            print('Host log:', machine.log_path, flush=True)
            machine.prepare()
            if args.action == 'doctor':
                machine.script('Doctor.ps1')
            elif args.action == 'setup':
                machine.setup()
            elif args.action == 'build':
                machine.setup()
                machine.build(args)
            else:
                machine.run(args.project)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print('ERROR:', error, file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
