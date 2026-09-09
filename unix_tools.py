"""Native macOS/Linux prerequisite diagnosis and repeatable installation."""
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request


def system_name():
    names = {'Darwin': 'macos', 'Linux': 'linux'}
    if platform.system() not in names or platform.machine() not in ('arm64', 'aarch64'):
        raise RuntimeError('This native machine definition requires macOS or Linux ARM64.')
    return names[platform.system()]


def command(args, env=None, cwd=None, capture=False):
    print('> ' + ' '.join(str(arg) for arg in args), flush=True)
    result = subprocess.run([str(arg) for arg in args], env=env, cwd=cwd,
                            stdout=subprocess.PIPE if capture else None,
                            stderr=subprocess.STDOUT if capture else None, text=True)
    if result.returncode:
        raise RuntimeError('Command failed (%s): %s\n%s' % (result.returncode, args, result.stdout or ''))
    return result.stdout.strip() if capture else ''


class Tools:
    def __init__(self, config):
        self.config = config
        self.os = system_name()
        self.profile = config['platforms'][self.os]
        self.root = Path.home() / '.local' / 'share' / 'build-machine'
        node_platform = 'darwin' if self.os == 'macos' else 'linux'
        self.node = self.root / ('node-v%s-%s-arm64' % (config['node']['version'], node_platform))
        self.pnpm = self.root / ('pnpm-' + config['pnpm']['version'])
        self.go = self.root / ('go-' + config['go']['version'])
        self.rustup = Path.home() / '.cargo' / 'bin' / 'rustup'
        self.env = os.environ.copy()
        self.env['PATH'] = os.pathsep.join(str(path) for path in (
            self.node / 'bin', self.pnpm / 'bin', self.go / 'go' / 'bin', self.rustup.parent,
            Path.home() / 'go' / 'bin')) + os.pathsep + self.env.get('PATH', '')
        self.env['RUSTUP_TOOLCHAIN'] = config['rust']['version']
        self.env['CI'] = 'true'
        self.env['NO_COLOR'] = '1'

    def missing_system_packages(self):
        if self.os == 'macos':
            for args in (['xcrun', '--find', 'clang'], ['xcrun', '--show-sdk-path']):
                if subprocess.run(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
                    return ['Apple Command Line Tools']
            return []
        missing = []
        for package in ['git', 'python3'] + self.profile['packages']:
            result = subprocess.run(['dpkg-query', '-W', '-f=${Status}', package], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            if result.returncode or result.stdout != 'install ok installed':
                missing.append(package)
        return missing

    def doctor(self):
        missing = self.missing_system_packages()
        versions = {}
        expected = {
            'node': (['node', '--version'], 'v' + self.config['node']['version']),
            'pnpm': (['pnpm', '--version'], self.config['pnpm']['version']),
            'rust': (['rustc', '--version'], 'rustc ' + self.config['rust']['version'] + ' '),
            'go': (['go', 'version'], 'go version go' + self.config['go']['version'] + ' '),
            'git': (['git', '--version'], 'git version '),
        }
        for name, (args, prefix) in expected.items():
            executable = shutil.which(args[0], path=self.env['PATH'])
            if not executable:
                versions[name] = None
                missing.append(name)
                continue
            result = subprocess.run([executable] + args[1:], env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            versions[name] = result.stdout.strip()
            if result.returncode or not versions[name].startswith(prefix):
                missing.append(name)
        os_version = platform.mac_ver()[0] if self.os == 'macos' else platform.freedesktop_os_release().get('PRETTY_NAME')
        report = {'platform': self.os, 'osVersion': os_version, 'architecture': platform.machine(), 'tools': versions,
                  'missing': sorted(set(missing)), 'ready': not missing}
        return report

    def download(self, url, expected, name):
        directory = self.root / 'downloads'
        directory.mkdir(parents=True, exist_ok=True)
        destination = directory / name
        if destination.exists() and hashlib.sha256(destination.read_bytes()).hexdigest() == expected:
            return destination
        partial = destination.with_name(destination.name + '.partial')
        print('Downloading ' + url, flush=True)
        with urllib.request.urlopen(url, timeout=60) as response, partial.open('wb') as output:
            shutil.copyfileobj(response, output)
        if hashlib.sha256(partial.read_bytes()).hexdigest() != expected:
            raise RuntimeError('Checksum mismatch: ' + url)
        partial.replace(destination)
        return destination

    def extract(self, archive, destination, nested=None):
        self.root.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='.extract-', dir=self.root) as temporary:
            temporary = Path(temporary)
            with tarfile.open(archive) as source:
                for member in source.getmembers():
                    try:
                        (temporary / member.name).resolve().relative_to(temporary)
                    except ValueError:
                        raise RuntimeError('Archive path escapes extraction directory.')
                source.extractall(temporary)
            extracted = temporary / nested if nested else temporary
            if destination.exists():
                raise RuntimeError('Incomplete managed installation at %s; inspect before replacement.' % destination)
            extracted.rename(destination)

    def setup_system(self):
        missing = self.missing_system_packages()
        if not missing:
            print('OK: native system dependencies present. No installation.', flush=True)
            return
        if self.os == 'macos':
            command(['xcode-select', '--install'])
            raise RuntimeError('Apple Command Line Tools installation was opened. Finish that system installer, then rerun the same command.')
        if os.geteuid() != 0:
            raise RuntimeError('Linux system packages require root. The Parallels controller runs setup-system as root.')
        env = os.environ.copy()
        env['DEBIAN_FRONTEND'] = 'noninteractive'
        command(['apt-get', 'update'], env=env)
        command(['apt-get', 'install', '-y'] + missing, env=env)

    def setup_user(self):
        if not (self.node / 'bin' / 'node').exists():
            archive = self.download(self.profile['nodeUrl'], self.profile['nodeSha256'], self.profile['nodeUrl'].rsplit('/', 1)[1])
            self.extract(archive, self.node, self.node.name)
        else:
            print('OK: declared Node.js already installed. No installation.', flush=True)
        if not (self.pnpm / 'bin' / 'pnpm').exists():
            command([self.node / 'bin' / 'npm', 'install', '--global', 'pnpm@' + self.config['pnpm']['version'], '--prefix', self.pnpm], env=self.env)
        else:
            print('OK: declared pnpm already installed. No installation.', flush=True)
        if not self.rustup.exists():
            installer = self.download(self.profile['rustupUrl'], self.profile['rustupSha256'], 'rustup-init-' + self.os)
            installer.chmod(0o755)
            command([installer, '-y', '--profile', 'minimal', '--default-toolchain', self.config['rust']['version'], '--no-modify-path'], env=self.env)
        else:
            installed = command([self.rustup, 'toolchain', 'list'], env=self.env, capture=True)
            if not any(line.startswith(self.config['rust']['version'] + '-') for line in installed.splitlines()):
                command([self.rustup, 'toolchain', 'install', self.config['rust']['version'], '--profile', 'minimal'], env=self.env)
            else:
                print('OK: declared Rust already installed. No installation.', flush=True)
        targets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'] if self.os == 'macos' else [self.profile['target']]
        installed = command([self.rustup, 'target', 'list', '--installed', '--toolchain', self.config['rust']['version']], env=self.env, capture=True).splitlines()
        for target in targets:
            if target not in installed:
                command([self.rustup, 'target', 'add', target, '--toolchain', self.config['rust']['version']], env=self.env)
        if not (self.go / 'go' / 'bin' / 'go').exists():
            archive = self.download(self.profile['goUrl'], self.profile['goSha256'], self.profile['goUrl'].rsplit('/', 1)[1])
            self.extract(archive, self.go)
        else:
            print('OK: declared Go already installed. No installation.', flush=True)
        report = self.doctor()
        print(json.dumps(report, indent=2), flush=True)
        if not report['ready']:
            raise RuntimeError('Native tool diagnosis failed: ' + ', '.join(report['missing']))
