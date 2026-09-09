# Windows 빌드 머신

[English](README.md)

Parallels의 Windows ARM64 개발 환경을 정의하는 저장소입니다. Codex 없이 일반 macOS 터미널에서 명령을 실행할 수 있습니다. 프로젝트들이 Node.js, pnpm, Rust, Go, Git, MSVC, Windows SDK를 함께 사용하고, 각 프로젝트의 소스·빌드 결과·로그는 Windows 디스크의 별도 디렉터리에 보관합니다.

## 동작 기준

- `setup`은 설정된 버전과 필수 MSVC 구성 요소를 확인하고 누락된 도구를 설치합니다. 기존 설치와 앱 데이터를 보존합니다. 재실행하면 설정을 충족하는 도구를 다시 설치하거나 PATH 항목을 중복 추가하지 않습니다.
- `doctor`는 실제 Windows 환경과 부족한 항목을 보여줍니다.
- `build`도 같은 필수 도구 검사·설치 절차를 자동 실행합니다. 별도의 준비 단계를 기억해 실행할 필요가 없습니다.
- `build PROJECT`는 현재 Git 추적 파일과 무시되지 않은 미추적 파일을 복사합니다. 커밋하지 않은 수정도 포함합니다. 무시된 파일은 제외하고, 프로젝트 밖으로 향하는 심볼릭 링크는 거부합니다. Git 리비전, 소스 체크섬, 도구 버전, 빌드 명령, 로그 위치, 실행 파일 체크섬을 기록합니다.
- 소스는 내용별로 별도 보관하며 Windows 디스크에서 빌드합니다. 소스·빌드 절차·머신 설정·실행 파일 체크섬이 일치하는 성공 결과만 재사용합니다.
- `run PROJECT`는 마지막으로 성공한 실행 파일을 로그인한 Windows 사용자의 화면에서 실행합니다. 반복 실행하면 같은 실행 파일의 실행 중인 프로세스를 재사용합니다. 프로세스와 창 확인은 실행 검증이며 모든 앱 기능을 보증하지는 않습니다.
- 표준 Tauri 2 프로젝트는 잠긴 의존성으로 릴리스 실행 파일을 만듭니다. Wails 2 프로젝트는 Go 모듈에 지정된 버전의 CLI를 사용합니다. 다른 구조는 Windows 빌드 명령과 실행 파일의 상대 경로를 직접 지정할 수 있습니다.
- 명령과 로그는 열어볼 수 있는 파일로 남깁니다. 자격 증명, 대화 기억, 어시스턴트 전용 서비스에 의존하지 않습니다.

## 명령

맥에는 Python 3, Git, `prlctl exec`를 지원하는 Parallels와 Parallels Tools가 필요합니다. Windows가 실행 중이고 데스크톱 사용자가 로그인해 있어야 합니다. VM 이름과 도구 버전은 [machine.json](machine.json)에 있습니다.

```sh
cd ~/Work/windows-build-machine
python3 winbuild.py setup
python3 winbuild.py doctor
python3 winbuild.py build ~/Work/airdata --run
python3 winbuild.py run ~/Work/airdata
```

프로젝트 디렉터리만 바꿔 같은 명령을 사용합니다. `--vm NAME`으로 다른 VM을 선택합니다. 사용자 지정 프로젝트는 `build`에 `--framework custom --command 'WINDOWS BUILD COMMAND' --artifact 'RELATIVE/PATH.exe'`를 전달합니다. 지정한 명령은 해당 프로젝트의 소스 디렉터리에서 데스크톱 사용자 권한으로 실행합니다.

전송에는 `WindowsBuildMachine`이라는 읽기 전용 Parallels 공유 폴더를 사용합니다. 제어 스크립트도 Windows에 복사하므로 Windows PowerShell에서 직접 열어보고 실행할 수 있습니다. 맥의 전송 파일·실행 기록·로그는 Git에서 제외한 `.state/`에 있습니다. Windows의 프로젝트와 빌드 기록은 `C:\BuildMachine\projects`에 있습니다. MSVC는 `C:\BuildTools`, 사용자 도구는 `%LOCALAPPDATA%\WindowsBuildMachine`에 설치합니다.

MSVC는 Microsoft가 유지보수하는 Visual Studio 2022 채널을 사용하며 필수 구성 요소를 확인합니다. 다른 도구 버전과 다운로드 체크섬은 `machine.json`에 고정합니다. 구성 요소를 충족하는 기존 MSVC 설치는 자동 업그레이드하지 않습니다. 반복 가능한 환경 구성이며, 컴파일 결과의 바이트 단위 동일성을 보장한다는 의미는 아닙니다.

## 검증

`python3 -m unittest discover -s tests`를 실행합니다. 실제 환경의 완료 기준은 setup 두 번 성공, Windows에서 실제 빌드 및 앱 창 실행, 반복 빌드·실행 시 검증된 결과와 실행 중인 프로세스 재사용입니다. 실제 결과와 아직 실행 검증하지 않은 프레임워크는 [verification.md](verification.md)에 기록합니다.

## 공식 설치 문서

- [Tauri 필수 도구](https://v2.tauri.app/start/prerequisites/)
- [MSVC 구성 요소 ID](https://learn.microsoft.com/en-us/visualstudio/install/workload-component-id-vs-build-tools?view=vs-2022), [설치 명령](https://learn.microsoft.com/en-us/visualstudio/install/use-command-line-parameters-to-install-visual-studio?view=vs-2022)
- [Rust 설치](https://rust-lang.org/tools/install/), [Node.js 다운로드](https://nodejs.org/en/download), [Go 설치](https://go.dev/doc/install), [Git for Windows](https://git-scm.com/install/windows)
- [Wails 2 설치](https://wails.io/docs/gettingstarted/installation/)
