#!/usr/bin/env python3
"""Provision and operate the Parallels Windows build machine without an agent."""
import argparse
import base64
import datetime
import hashlib
import json
import os
if os.name != 'nt':
    import fcntl
from pathlib import Path
import re
import shlex
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


def git_root(project):
    """Return the repository root and reject subdirectories as projects."""
    project = Path(project).expanduser().resolve()
    try:
        root = Path(git(project, 'rev-parse', '--show-toplevel').decode().strip()).resolve()
    except (OSError, subprocess.CalledProcessError):
        raise ValueError('프로젝트 루트가 Git 저장소가 아니에요.')
    if project != root:
        raise ValueError('프로젝트는 Git 저장소 루트만 등록할 수 있어요. 모노레포 하위 앱은 workflow의 working-directory를 사용하세요.')
    return root


def make_source_archive(project, destination, ref=None):
    """Archive a local working tree or an immutable Git ref."""
    project = git_root(project)
    revision = git(project, 'rev-parse', ref or 'HEAD').decode().strip()
    if ref:
        entries = []
        raw_entries = git(project, 'ls-tree', '-r', '-z', revision).split(b'\0')
        for raw in raw_entries:
            if not raw:
                continue
            metadata, name = raw.split(b'\t', 1)
            mode, kind, _ = metadata.decode().split(' ', 2)
            if kind != 'blob' or mode.startswith('120'):
                raise ValueError('고정 ref에 지원하지 않는 Git 항목이 있어요: ' + os.fsdecode(name))
            entries.append((os.fsdecode(name), mode, git(project, 'show', f'{revision}:{os.fsdecode(name)}')))
        dirty = False
    else:
        files = sorted(set(git(project, 'ls-files', '-z', '--cached', '--others', '--exclude-standard').split(b'\0')) - {b''})
        entries = []
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
            entries.append((name, format(path.stat().st_mode & 0o777, 'o'), path.read_bytes()))
        dirty = bool(git(project, 'status', '--porcelain'))
    destination.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(destination, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        for name, mode, data in sorted(entries, key=lambda value: value[0]):
            entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            entry.compress_type = zipfile.ZIP_DEFLATED
            entry.external_attr = (int(mode, 8) & 0xFFFF) << 16
            archive.writestr(entry, data)
    return {'revision': revision, 'dirty': dirty, 'sourceHash': digest(destination.read_bytes()), 'fileCount': len(entries), 'sourceMode': 'ref' if ref else 'local'}


def controller_files():
    return [ROOT / 'machine.json'] + sorted((ROOT / 'windows').glob('*.ps1'))


def controller_hash():
    return digest(b''.join(path.relative_to(ROOT).as_posix().encode() + b'\0' + path.read_bytes() + b'\0' for path in controller_files()))


def snapshot_project(args):
    project = args.project.expanduser().resolve()
    git_root(project)
    key = project_key(project)
    transfer = STATE / 'projects' / key
    transfer.mkdir(parents=True, exist_ok=True)
    temporary = transfer / 'source.pending.zip'
    source = make_source_archive(project, temporary, getattr(args, 'ref', None))
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
            raise ValueError('Use --framework custom --command COMMAND --artifact RELATIVE_PATH for this project.')
    if framework == 'custom' and (not args.command or not args.artifact):
        raise ValueError('Custom builds require --command and --artifact.')
    return dict(source, framework=framework, command=args.command, artifact=args.artifact,
                projectKey=key, project=str(project), archive=str(archive))


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
        command = [self.prlctl, *args]
        with self.log_path.open('ab') as log:
            log.write(('> ' + shlex.join(command) + '\n').encode())
            log.flush()
            result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            log.write(result.stdout)
            log.write(('\nExit code: %s\n' % result.returncode).encode())
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
            log.write(('> Windows PowerShell (%s): %s\n' % ('SYSTEM' if system else 'desktop user', script)).encode())
            log.flush()
            chunks = []
            with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT) as process:
                for line in iter(process.stdout.readline, b''):
                    chunks.append(line)
                    log.write(line)
                    log.flush()
                    print(decode_output(line).rstrip(), flush=True)
                code = process.wait()
            log.write(('Exit code: %s\n' % code).encode())
        if code:
            output = decode_output(b''.join(chunks)).strip()
            details = [line.removeprefix('ERROR: ') for line in output.splitlines() if line.startswith('ERROR: ')]
            reason = '\n'.join(details) if details else output[-2500:]
            raise RuntimeError('Windows command failed with exit code %s.\n%s\nLog: %s' % (code, reason, self.log_path))

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

    def build(self, args, snapshot=None):
        source = snapshot or snapshot_project(args)
        project = Path(source['project'])
        key = source['projectKey']
        transfer = STATE / 'projects' / key
        archive = Path(source['archive'])
        recipe = {key: source[key] for key in ('framework', 'command', 'artifact')}
        build_id = digest(json.dumps({'source': source['sourceHash'], 'controller': self.control_hash, 'recipe': recipe}, sort_keys=True).encode())
        request = dict(source, buildId=build_id, controllerHash=self.control_hash,
                       archive='\\\\Mac\\' + self.config['share'] + '\\' + str(archive.relative_to(ROOT)).replace('/', '\\'))
        request_path = transfer / (build_id + '.json')
        request_path.write_text(json.dumps(request, indent=2) + '\n')
        remote_request = '\\\\Mac\\' + self.config['share'] + '\\' + str(request_path.relative_to(ROOT)).replace('/', '\\')
        self.script('Build.ps1', '-RequestPath ' + ps_quote(remote_request))
        (transfer / 'latest.json').write_text(json.dumps(request, indent=2) + '\n')
        if args.run:
            self.run(project)

    def ci(self, args, snapshot, config):
        data = dict(snapshot)
        data['archive'] = '\\\\Mac\\' + self.config['share'] + '\\' + str(Path(data['archive']).relative_to(ROOT)).replace('/', '\\')
        data['target'] = config['platforms']['windows']['target']
        data['bundle'] = config['platforms']['windows'].get('bundle')
        data['ci'] = True
        transfer = STATE / 'projects' / data['projectKey']
        transfer.mkdir(parents=True, exist_ok=True)
        request_path = transfer / 'windows-ci-request.json'
        result_path = transfer / 'windows-ci-result.json'
        if result_path.exists():
            result_path.unlink()
        data['resultPath'] = '\\\\Mac\\' + self.config['share'] + '\\' + str(result_path.relative_to(ROOT)).replace('/', '\\')
        request_path.write_text(json.dumps(data, indent=2) + '\n')
        remote_request = '\\\\Mac\\' + self.config['share'] + '\\' + str(request_path.relative_to(ROOT)).replace('/', '\\')
        self.script('Build.ps1', '-RequestPath ' + ps_quote(remote_request) + ' -CI')
        if not result_path.exists():
            raise RuntimeError('Windows workflow did not write a structured result.')
        return json.loads(result_path.read_text())

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
