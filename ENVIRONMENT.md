# 로컬 환경 구성과 복구

이 문서는 macOS 호스트에서 기존 Parallels Windows/Ubuntu 머신을 사용하는 개발 환경을 설명한다. VM 생성과 OS 설치는 Parallels에서 먼저 완료한다. `machine.json`이 머신 이름과 도구 버전의 기준이며, 비밀번호는 저장하지 않는다.

## 머신 구성

| 환경 | 설정 |
|---|---|
| 호스트 | macOS, Git, Rust (`Cargo.toml`의 최소 버전 이상), Node.js, pnpm, 활성화된 Parallels Desktop |
| Windows | VM 이름 `Windows 11`, ARM64, Parallels Tools 설치, 데스크톱 로그인 |
| Ubuntu | VM 이름 `Ubuntu 26.04 ARM64`, ARM64, 계정 `parallels`, Parallels Tools 설치, 데스크톱 로그인 |
| 소스 전달 | `WindowsBuildMachine`이라는 읽기 전용 호스트 공유 폴더 |

VM 이름이나 Ubuntu 계정이 다르면 `machine.json`의 `platforms` 설정을 변경한다. Ubuntu 계정은 `desktopUser`로 명시한다. 공유 폴더는 컨트롤러가 준비하며 현재 컨트롤러 폴더를 가리켜야 한다. 같은 공유 이름이 다른 체크아웃을 가리키면 오류로 중단한다. Windows는 `\\Mac\WindowsBuildMachine`, Ubuntu는 `/media/psf/WindowsBuildMachine`에서 읽는다. 빌드 출력은 게스트 로컬 디스크에 저장한다.

## 연결 확인

CLI가 PATH에 없다면 아래 전체 경로를 사용한다.

```sh
PRLCTL='/Applications/Parallels Desktop.app/Contents/MacOS/prlctl'
"$PRLCTL" list --all --json
"$PRLCTL" exec 'Ubuntu 26.04 ARM64' 'id'
"$PRLCTL" exec 'Ubuntu 26.04 ARM64' 'runuser -l parallels -c '\''id; echo "$HOME"; pwd'\'''
"$PRLCTL" exec 'Windows 11' --current-user whoami
```

Ubuntu의 첫 호출은 root, 두 번째 호출은 `parallels`와 `/home/parallels`를 출력해야 한다. Linux 컨트롤러는 Parallels의 root 실행 채널에서 `runuser -l <desktopUser> -c ...`로 워커를 실행한다. 계정의 비밀번호를 제거하거나 저장하지 않는다. 시스템 패키지 설치는 root 채널을 사용한다. Windows는 기존 `--current-user` 경로를 사용한다. Linux에서 `desktopUser`가 없으면 기존 `--current-user` 경로로 동작한다.

## 앱 실행과 도구 준비

Ubuntu에서 `setup`은 `git`, `python3`와 `machine.json`의 Linux `packages`를 apt로 설치한다. 여기에는 C/C++ 빌드 도구, pkg-config, WebKitGTK 4.1·OpenSSL·XDo·Ayatana AppIndicator·SVG 개발 패키지, deb 패키징 도구, Xvfb·xauth·wmctrl이 포함된다. 사용자 도구는 Node.js 22.23.2, pnpm 11.24.0, Rust 1.98.1, Go 1.27.1이다. 버전의 최종 기준은 `machine.json`이며 다운로드는 선언된 SHA-256으로 확인한다. 설치 과정에서 apt가 의존 패키지를 함께 설치하거나 업그레이드할 수 있다.

Linux 사용자 도구는 `/home/parallels/.local/share/build-machine`와 `/home/parallels/.cargo` 등에 설치된다. 프로젝트별 작업 디렉터리는 `/home/parallels/.local/state/build-machine`이다. 시스템 전체 도구와 사용자 도구의 설치 권한을 구분한다. 데스크톱 준비는 GDM 자동 로그인과 화면 꺼짐·잠금 방지를 구성한다. GDM을 변경할 때 기존 설정은 `.conf.before-build-machine` 백업으로 남기며, 이미 로그인된 세션은 재시작하지 않는다.

저장소에 준비된 `workers/`의 플랫폼별 워커가 필요하다. 배포 앱에는 워커가 포함된다. 개발 워커를 다시 만들 때에는 `cargo xtask worker --os windows linux`를 사용하며, 게스트에도 해당 워커를 컴파일할 Rust 환경이 필요하다.

```sh
cd ~/Work/build-machine
cargo xtask dev
```

개발 모드는 Vite 프런트엔드와 네이티브 앱을 함께 실행한다. `target/debug/build-machine-desktop`만 실행하면 개발 프런트엔드 서버가 없어 화면이 표시되지 않을 수 있다. 배포 앱을 만들고 열려면 `cargo xtask build --run`을 사용한다. 이 명령은 세 OS 워커를 다시 빌드하므로 게스트 빌드 환경도 필요하다.

앱의 Settings에서 컨트롤러 경로를 이 저장소로 지정하고 Ubuntu를 선택하여 진단한다. 누락된 도구는 Setup으로 준비한 다음 다시 진단한다. 프로젝트 등록 후 Build와 Run을 확인한다. 데스크톱에 앱 창이 표시되는지 별도로 확인해야 하며, 진단 성공만으로 프로젝트 실행 성공을 판단하지 않는다.

CLI에서 같은 흐름을 실행하려면:

```sh
cargo build --locked -p build-machine-controller
target/debug/build-machine --root "$PWD" doctor --os linux
target/debug/build-machine --root "$PWD" setup --os linux
target/debug/build-machine --root "$PWD" build /absolute/path/to/project --os linux --run
```

## 장애 확인 순서

1. VM이 `running`인지, Parallels Tools가 설치되어 있는지 확인한다.
2. Ubuntu에서 `loginctl list-sessions`로 `parallels`의 데스크톱 세션을 확인한다. 명령 실행 권한만으로 그래픽 세션이 생기지는 않는다.
3. `--current-user` 인증 오류는 데스크톱 로그인과 별개로 발생할 수 있다. Linux는 위 `runuser` 연결을 확인한다. 비밀번호를 비우는 방식으로 해결하지 않는다.
4. 공유 폴더의 이름·대상 경로·읽기 전용 설정과 게스트 마운트를 확인한다.
5. Settings의 진단 결과와 `.state/logs/`, `.state/runs/<run>/report.json`을 확인한다. 누락된 도구와 연결 실패를 구분한다.

설정은 `~/Library/Application Support/local.buildmachine.desktop/preferences.json`, 도구 진단 기록은 `.state/tool-status.json`에 있다. 이전 성공 기록은 현재 VM이 준비되어 있다는 증거가 아니다. 새 VM을 만들거나 설정을 바꾸면 다시 진단한다.

## GitHub Actions와의 호환성을 유지하는 절차

프로젝트의 `.github/workflows/*.yml`을 실행 계약으로 사용한다. 실행 전에 `ci validate`로 지원 여부를 검사하고, 준비된 머신에서 `ci run`으로 같은 파일을 재생한다. 워크플로를 통과시키기 위해 임의로 단계를 빼거나 성공으로 취급하지 않는다.

```sh
target/debug/build-machine --root "$PWD" ci validate /absolute/path/to/project --workflow .github/workflows/release.yml
target/debug/build-machine --root "$PWD" ci run /absolute/path/to/project --workflow .github/workflows/release.yml --os linux
```

지원 액션 목록은 `crates/core/src/workflow/adapter.rs`, 파싱과 표현식 검사는 `crates/core/src/workflow/parse.rs`, 실제 로컬 실행은 `crates/worker/src/ci.rs`가 기준이다. 현재 checkout, Node/pnpm/Rust 준비, 캐시, Tauri 빌드, 아티팩트와 릴리스에 로컬 어댑터가 있다. 해당 액션의 GitHub 구현을 그대로 실행하는 것은 아니므로 버전·입력·환경·캐시 동작의 차이를 확인해야 한다. 지원되지 않는 액션·컨테이너·서비스·표현식은 검증 실패로 드러나야 한다.

현재 구현의 구체적인 차이는 다음과 같다. Node/pnpm/Rust setup 액션은 `machine.json`으로 먼저 준비된 도구를 사용하며 액션 자체를 실행하지 않는다. 워크플로에 적힌 입력이 실제 설치 버전과 일치하는지 별도 확인해야 한다. 또한 실행기는 단계를 setup → test → build → smoke → release 순으로 묶어 실행하므로 원본 YAML의 단계 순서를 그대로 보장하지 않는다. 단계 간 순서에 의존하는 워크플로에서는 호환성 문제이며 유지보수 대상으로 남는다.

이번 `airdata` Linux 잡은 `runs-on: ubuntu-24.04-arm`을 선언하지만 실제 로컬 게스트는 Ubuntu 26.04 ARM64다. CPU 아키텍처와 도구의 주요 버전은 맞지만 OS 이미지와 시스템 라이브러리는 같지 않다. 로컬 통과를 Ubuntu 24.04 GitHub 러너에서의 통과로 간주하지 않는다. 러너 이미지 차이도 검증 기록에 포함한다.

유지보수할 때에는 소스 revision과 dirty 상태, 워크플로 경로, 이벤트/ref, 도구 버전, 실행 명령, 실패 단계, 산출물 및 보고서의 `limits`를 함께 기록한다. GitHub secrets·업로드·릴리스·서명과 로컬 재현의 차이를 숨기지 않는다. `passed_with_limits`는 GitHub에서의 완전한 성공과 같지 않다. 원격 배포나 실제 릴리스 발행은 별도 작업이다.

2026-09-30에 `/Users/maxkwon/Work/airdata`의 `.github/workflows/release.yml`은 revision `010c133782119c0ffb4e541c9727b6f4442f79db`, dirty=false로 `ci validate`를 통과했다. 세 OS 잡이 인식되었으며, 이 결과는 실행 성공을 의미하지 않는다.

## 이번 환경에서 확인한 범위 — 2026-09-30

Parallels Desktop 27.0.2, Ubuntu VM 실행, Parallels Tools 27.0.2 설치, `parallels` 데스크톱 로그인, root 실행 채널 및 `runuser`를 통한 비밀번호 없는 사용자 명령 실행을 확인했다. Linux의 `--current-user`는 인증에 실패했다. 기존 `setup` 실행 `20260930-183353-394136`과 후속 `doctor` 실행 `20260930-183516-469377`이 성공했다. Node 22.23.2, pnpm 11.24.0, Rust 1.98.1, Go 1.27.1, Git 2.53.0 및 모든 선언된 Linux 패키지가 준비되어 `ready=true`로 확인되었다. 네이티브 GUI 대시보드 화면도 확인했다. 실제 `airdata` Linux 워크플로 실행 `20260930-183524-943541`은 `passed_with_limits`로 완료되었다. 의존성 설치·TypeScript 검사·Rust 테스트 38개·프런트엔드 빌드·ARM64 DEB 생성이 통과했다. smoke는 워크플로에서 명시적으로 생략하며 서명·업로드는 검증되지 않았다. DEB SHA-256은 `fb536dd78c6836d72f579fa35f54ea62bff9aa74a1d3f9819f5da519ae27757b`이다. 상세 이력과 실패 범위는 [verification.md](verification.md)의 2026-09-30 기록을 따른다.

Ubuntu VM의 중첩 가상화 설정은 현재 꺼져 있다. 이 문서의 빌드 환경 연결 검증은 게스트 내부에서 KVM 등 다른 VM을 실행할 수 있다는 검증을 포함하지 않는다.
