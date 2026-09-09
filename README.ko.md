# 빌드 머신

[English](README.md)

Codex 없이 실행하고 내용을 확인할 수 있는 빌드 머신 명령을 제공합니다. 공통 목표는 [세 OS 릴리즈 검증 명세](PLATFORMS.ko.md)에 있습니다. Windows와 Ubuntu의 도구 준비, 진단, AIRDATA 빌드, 실제 창 표시를 확인했습니다. macOS는 도구 준비를 확인했고 앱과 설치 패키지 검증은 남아 있습니다. 각 운영체제는 정의한 도구를 프로젝트 간에 공유하고, 소스·산출물·로그를 프로젝트별 디렉터리에 보관합니다.

## 동작 기준

- `setup`은 설정된 버전과 필수 MSVC 구성 요소를 확인하고 누락된 도구를 설치합니다. 기존 설치와 앱 데이터를 보존합니다. 재실행하면 설정을 충족하는 도구를 다시 설치하거나 PATH 항목을 중복 추가하지 않습니다.
- `doctor`는 선택한 실제 환경과 부족한 항목을 보여줍니다.
- `build`도 같은 필수 도구 검사·설치 절차를 자동 실행합니다. 별도의 준비 단계를 기억해 실행할 필요가 없습니다.
- `build PROJECT`는 현재 Git 추적 파일과 무시되지 않은 미추적 파일을 복사합니다. 커밋하지 않은 수정도 포함합니다. 무시된 파일은 제외하고, 프로젝트 밖으로 향하는 심볼릭 링크는 거부합니다. Git 리비전, 소스 체크섬, 도구 버전, 빌드 명령, 로그 위치, 실행 파일 체크섬을 기록합니다.
- 소스는 내용별로 별도 보관하며 선택한 운영체제의 로컬 디스크에서 빌드합니다. 소스·빌드 절차·작업 스크립트·머신 설정·실행 파일 체크섬이 일치하는 성공 결과만 재사용합니다. 네이티브 작업 스크립트는 소스 스크립트의 실행 권한을 보존합니다.
- `run PROJECT`는 마지막으로 성공한 실행 파일을 로그인한 사용자의 화면에서 실행합니다. 반복 실행하면 같은 실행 파일의 실행 중인 프로세스를 재사용합니다. 프로세스와 창 확인은 실행 검증이며 모든 앱 기능을 보증하지는 않습니다.
- 표준 Tauri 2 프로젝트는 잠긴 의존성으로 릴리스 실행 파일을 만듭니다. Windows의 Wails 2 프로젝트는 Go 모듈에 지정된 버전의 CLI를 사용합니다. 다른 구조와 네이티브 Wails 프로젝트는 빌드 명령과 실행 파일의 상대 경로를 직접 지정할 수 있습니다.
- 명령과 로그는 열어볼 수 있는 파일로 남깁니다. 자격 증명, 대화 기억, 어시스턴트 전용 서비스에 의존하지 않습니다.
- 등록 프로젝트는 Git 저장소 루트입니다. 모노레포의 하위 앱은 workflow 단계의 `working-directory`로 선택하며 하위 디렉터리를 별도 프로젝트로 등록하지 않습니다.
- `ci validate`는 저장소 GitHub Actions workflow를 읽고 지원하지 않는 action·컨테이너·서비스·표현식이 있으면 실패합니다. `ci run`은 선택한 OS 작업자에서 지원하는 `uses`와 `run` 단계를 실행하고 checkout·캐시·산출물·릴리즈·서명은 로컬 어댑터로 대체합니다.
- Workflow 재현은 현재 작업 트리 또는 명시한 커밋·브랜치·태그를 사용합니다. 해석한 리비전, 변경 상태, 이벤트, 단계 명령, 단계 결과, 산출물 체크섬과 제한을 기록합니다. workflow에 test나 smoke 단계가 없으면 `# build-machine: skip <stage> reason=...` 주석으로 명시해야 합니다.

## 명령

[Tauri 2 데스크톱 앱](GUI.ko.md)은 등록 프로젝트, 최근 기록된 빌드 결과와 빌드 로그를 보여주는 대시보드로 시작합니다. 현재 GUI 작업을 표시하며 해당 작업 화면으로 돌아갈 수 있습니다. `+`로 Git 폴더를 등록하고 해당 항목을 선택해 빌드 대상 설정·빌드·실행을 합니다. 프로젝트별 옵션은 각각 저장합니다. 하단 **설정** 버튼에서 모든 프로젝트가 공유하는 Windows/Ubuntu/macOS 진단과 도구 자동 준비를 엽니다. 환경별 마지막 진단/준비 결과와 완료 시각은 앱을 다시 열어도 유지합니다. 연결 상태와 과거 도구 검사 결과는 구분해서 표시합니다. 지원 구조와 남은 프레임워크 검증 범위는 [프로젝트 빌드 조건](PROJECTS.ko.md)에 있습니다.

등록 목록과 GUI 설정은 `~/Library/Application Support/local.buildmachine.desktop/preferences.json`에 있습니다. **설정 → 설정 폴더 열기**로 파일을 확인할 수 있습니다. 프로젝트 등록 목록은 최근 빌드 로그와 독립적입니다. 등록을 해제해도 소스 폴더와 앱 데이터는 유지합니다.

```sh
cd ~/Work/build-machine
python3 desktop.py build --run
```

빌드 후에는 `gui/src-tauri/target/release/bundle/macos/Build Machine.app`을 직접 열면 됩니다. 첫 macOS 앱은 서명하지 않았으며 내부적으로 이 체크아웃과 Python이 필요합니다. `python3 desktop.py dev`는 React와 Rust/Tauri 개발을 함께 실행하고, `python3 desktop.py test`는 화면 빌드·Rust 연결·브라우저 동작을 검사합니다. GUI는 미완성인 릴리즈 게시 기능을 노출하지 않습니다.

맥에는 Python 3, Git, `prlctl exec`를 지원하는 Parallels와 Parallels Tools가 필요합니다. 선택한 VM이 실행 중이고 데스크톱 사용자가 로그인해 있어야 합니다. Linux에는 Python 3.10 이상이 필요하며 Ubuntu 26.04에는 Python 3.14가 있습니다. VM 이름과 도구 버전은 [machine.json](machine.json)에 있습니다.

공통 제어 명령은 `windows`, `linux`, `macos`, `all`을 선택합니다. `--os windows linux`처럼 여러 이름을 지정할 수 있으며, `--os`를 생략하면 세 운영체제를 선택합니다. 매트릭스 실행은 기본적으로 순차 실행이고 `--execution parallel`을 지정하면 선택한 OS를 동시에 실행합니다. 병렬 실행도 단계·명령·로그·재시도·산출물·실패 원인을 순차 실행과 같은 필드에 기록합니다. 하나의 소스 스냅샷을 사용하고 모든 선택 플랫폼의 결과를 기다린 뒤 하나라도 실패하면 전체 명령도 실패합니다. `--result-file PATH`는 다른 인터페이스를 위한 구조화된 결과를 기록합니다. 진단/준비 결과와 시각은 `.state/tool-status.json`에 유지하며 머신 설정이 바뀌면 이전 결과를 표시하지 않습니다.

```sh
cd ~/Work/build-machine
python3 build.py doctor --os linux
python3 build.py setup --os linux
python3 build.py build ~/Work/airdata --os linux --run
python3 build.py run ~/Work/airdata --os linux

# 저장소 workflow 재현. 생략한 test/smoke는 위 skip 주석으로 명시해야 합니다.
python3 build.py ci validate ~/Work/airdata --workflow .github/workflows/release-macos.yml --event workflow_dispatch
python3 build.py ci run ~/Work/airdata --workflow .github/workflows/release-macos.yml --event workflow_dispatch --ref v0.1.0 --execution parallel --os macos
```

`build`는 누락된 필수 도구를 자동 설치하므로 별도의 `setup`은 선택 사항입니다. Linux 시스템 패키지는 Parallels를 통해 root로 설치하며, 빌드와 앱 실행은 로그인한 데스크톱 사용자로 수행합니다. 실행 명령은 해당 사용자의 systemd 환경에서 데스크톱 연결 설정만 읽고 프로세스가 계속 실행되는지 검사합니다. 실제 화면 표시는 별도 화면 캡처로 확인합니다.

기존 Windows 전용 진입점도 사용할 수 있습니다.

```sh
cd ~/Work/build-machine
python3 winbuild.py setup
python3 winbuild.py doctor
python3 winbuild.py build ~/Work/airdata --run
python3 winbuild.py run ~/Work/airdata
```

프로젝트 디렉터리만 바꿔 같은 명령을 사용합니다. `winbuild.py --vm NAME`은 다른 Windows VM을 선택하며 공통 제어 명령은 `machine.json`을 사용합니다. 사용자 지정 프로젝트는 `build`에 `--framework custom --command 'BUILD COMMAND' --artifact 'RELATIVE/PATH'`를 전달합니다. 지정한 명령은 해당 프로젝트의 소스 스냅샷에서 선택한 플랫폼의 셸을 사용해 데스크톱 사용자 권한으로 실행합니다.

전송에는 `WindowsBuildMachine`이라는 읽기 전용 Parallels 공유 폴더를 사용합니다. 제어 스크립트도 Windows에 복사하므로 Windows PowerShell에서 직접 열어보고 실행할 수 있습니다. 맥의 전송 파일·실행 기록·로그는 Git에서 제외한 `.state/`에 있습니다. Windows의 프로젝트와 빌드 기록은 `C:\BuildMachine\projects`에 있습니다. MSVC는 `C:\BuildTools`, 사용자 도구는 `%LOCALAPPDATA%\WindowsBuildMachine`에 설치합니다.

Linux는 같은 공유 폴더를 `/media/psf/WindowsBuildMachine`에서 읽습니다. Linux와 macOS는 관리하는 사용자 도구를 `~/.local/share/build-machine`에, 빌드 기록을 `~/.local/state/build-machine/PROJECT_KEY/latest.json`에 보관합니다. 네이티브 준비 명령은 시스템 기본 Node.js와 Go 설치를 보존합니다. Ubuntu의 Tauri 빌드는 ARM64 실행 파일과 `.deb`를 만들지만, 패키지 생성이 시스템 패키지 설치까지 검증한 것은 아닙니다. Workflow 재현은 서명하지 않은 로컬 패키징과 로컬 산출물·릴리즈 어댑터를 검증하며 서명·공증·GitHub 업로드·상태 보고는 명시적으로 미검증으로 기록합니다.

MSVC는 Microsoft가 유지보수하는 Visual Studio 2022 채널을 사용하며 필수 구성 요소를 확인합니다. 다른 도구 버전과 다운로드 체크섬은 `machine.json`에 고정합니다. 구성 요소를 충족하는 기존 MSVC 설치는 자동 업그레이드하지 않습니다. 반복 가능한 환경 구성이며, 컴파일 결과의 바이트 단위 동일성을 보장한다는 의미는 아닙니다.

## 검증

네이티브 GUI는 `python3 tests/native_gui.py doctor --os windows linux macos` 또는 `python3 tests/native_gui.py setup --os linux --restart`로 검사합니다. 실제 macOS 앱 컨트롤을 누르고 실제 결과를 검사한 뒤 앱 창을 캡처합니다. 명령을 실행하는 앱에 macOS 손쉬운 사용과 화면 캡처 접근이 필요합니다. 준비 검사는 정의된 누락 도구를 설치할 수 있습니다. 다시 연 화면의 결과 표시는 캡처를 확인해야 하며 브라우저 모의만으로 네이티브 동작이 확인되지는 않습니다.

로컬 검사에서 Parallels 27.0.1의 `prlctl exec`가 간헐적으로 실패했습니다. `python3 tests/parallels_smoke.py --transport prlctl --iterations 30`은 무해한 게스트 명령으로 사용자·출력·종료 상태를 검사합니다. 반복 가능한 이 검사는 실행 도구를 고치지는 않습니다. 실패한 게스트 작업은 자동 재실행하지 않고 실패로 보고합니다. 관찰한 오류와 한계는 [verification.md](verification.md)에 기록합니다.

macOS 또는 Linux에서 `python3 -m unittest discover -s tests`를 실행합니다. 실제 환경의 완료 기준은 반복 도구 준비 성공, 실제 빌드와 앱 창 표시, 반복 빌드·실행 시 검증된 결과와 실행 중인 프로세스 재사용입니다. 실제 결과와 아직 실행 검증하지 않은 플랫폼·프레임워크는 [verification.md](verification.md)에 기록합니다.

## 공식 설치 문서

- [Tauri 필수 도구](https://v2.tauri.app/start/prerequisites/)
- [MSVC 구성 요소 ID](https://learn.microsoft.com/en-us/visualstudio/install/workload-component-id-vs-build-tools?view=vs-2022), [설치 명령](https://learn.microsoft.com/en-us/visualstudio/install/use-command-line-parameters-to-install-visual-studio?view=vs-2022)
- [Rust 설치](https://rust-lang.org/tools/install/), [Node.js 다운로드](https://nodejs.org/en/download), [Go 설치](https://go.dev/doc/install), [Git for Windows](https://git-scm.com/install/windows)
- [Wails 2 설치](https://wails.io/docs/gettingstarted/installation/)
