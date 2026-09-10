# 빌드 머신

[English](README.md)

Codex 없이 동작하는, 열어볼 수 있는 빌드 머신 명령입니다. 공통 목표는 [세 운영체제 릴리즈 리허설 명세](PLATFORMS.ko.md)가 정의합니다. 각 운영체제는 선언된 도구를 프로젝트들이 공유하고, 소스·출력·로그는 프로젝트별 디렉터리에 따로 보관합니다.

Rust 워크스페이스 하나입니다. 컨트롤러는 이 맥에서 돌고, 대상마다 워커 바이너리가 돕니다. 프로젝트를 빌드하는 기계에는 Python도 PowerShell도, 자체 Rust 툴체인도 필요 없습니다 — 릴리즈가 실어 보낸 워커를 실행할 뿐입니다.

## 요구 동작

- `setup`은 선언된 버전과 필요한 MSVC 구성 요소를 확인하고, 없는 도구만 설치하며, 기존 설치와 애플리케이션 데이터를 보존합니다. 다시 실행해도 조건을 만족하는 도구를 재설치하거나 PATH를 중복 추가하지 않습니다.
- `doctor`는 실제 선택된 환경과 빠진 요구사항을 보고합니다.
- `build`는 같은 사전 점검과 설치 절차를 자동으로 수행합니다. 별도로 기억해야 할 setup 단계가 없습니다.
- `build PROJECT`는 현재 Git 추적 파일과 무시되지 않은 미추적 파일을, 커밋하지 않은 수정까지 포함해 복사합니다. 무시된 파일은 제외하고 프로젝트 밖을 가리키는 심볼릭 링크는 거부합니다. Git 리비전, 소스 체크섬, 도구 버전, 빌드 명령, 로그 경로, 실행 파일 체크섬을 기록합니다.
- 소스 스냅샷은 내용 기준으로 따로 보관합니다. 빌드는 선택한 운영체제의 로컬 디스크에서 수행합니다. 성공한 결과는 소스·레시피·워커 바이너리·머신 설정·실행 파일 체크섬이 모두 일치할 때만 재사용합니다. 워커는 소스 스크립트의 실행 권한을 보존합니다.
- `run PROJECT`은 마지막으로 성공한 실행 파일을 로그인된 데스크톱에서 시작합니다. 반복하면 그 실행 파일의 실행 중인 프로세스를 재사용합니다. 프로세스·창 확인으로 실행을 검증하며, 애플리케이션의 모든 기능을 보증하지는 않습니다.
- 표준 Tauri 2 프로젝트는 잠긴 의존성을 사용해 릴리즈 실행 파일을 만듭니다. 다른 배치는 명시적 빌드 명령과 실행 파일 경로를 지정할 수 있습니다.
- 명령과 로그는 확인할 수 있습니다. 소스는 공개돼 있고, 릴리즈는 이 저장소의 공개 워크플로가 빌드하며, 모든 실행은 무엇을 실행했고 출력을 어디에 남겼는지 기록합니다.
- 등록하는 프로젝트는 Git 저장소 루트입니다. 모노레포의 하위 앱은 workflow 단계의 `working-directory`로 선택하며, 하위 디렉터리를 별도 프로젝트로 등록할 수 없습니다.
- `ci validate`는 저장소의 GitHub Actions workflow를 읽고, 지원하지 않는 action·컨테이너·서비스·표현식에 대해 실패로 닫습니다. `ci run`은 지원하는 `uses`와 `run` 단계를 선택한 OS 워커에서 실행하며 checkout·캐시·산출물·릴리스·서명은 로컬 어댑터로 대체합니다.
- 워크플로 재현은 현재 작업 트리 또는 명시한 커밋·브랜치·태그를 받습니다. 확정된 리비전, 변경 여부, 이벤트, 단계 명령, 스테이지 결과, 산출물 체크섬, 제한을 기록합니다. workflow에 test나 smoke 단계가 없으면 `# build-machine: skip <stage> reason=...` 주석이 반드시 있어야 합니다. 지원 action을 쓰는 단계는 그 어댑터로 분류하므로, action의 이름이 스테이지 게이트를 대신 충족시키는 일은 없습니다.

## 명령

[Tauri 2 데스크톱 앱](GUI.ko.md)은 등록된 프로젝트와 최근 빌드 결과·로그를 보여주는 대시보드로 열립니다. `+`로 Git 폴더를 등록한 뒤 항목을 선택해 빌드 대상을 설정하고 빌드·실행합니다. 프로젝트마다 자기 옵션을 유지합니다. 하단 **설정** 버튼은 모든 프로젝트가 공유하는 Windows/Ubuntu/macOS 진단과 자동 도구 준비를 엽니다. 당신의 저장소에서 무엇을 바꿔야 하는지는 [빌드 머신 채택하기](ADOPTING.ko.md)에, 지원하는 배치와 남은 프레임워크 범위는 [프로젝트 빌드 요구사항](PROJECTS.ko.md)에 있습니다.

등록과 GUI 설정은 `~/Library/Application Support/local.buildmachine.desktop/preferences.json`에 있습니다. **설정 → 설정 폴더 열기**로 확인할 수 있습니다.

```sh
cd ~/Work/build-machine
cargo xtask build --run
```

`cargo xtask dev`는 프런트엔드와 Rust 앱을 함께 시작하고, `cargo xtask test`는 Rust 테스트·clippy·프런트엔드 빌드·브라우저 테스트를 실행합니다. `cargo xtask worker --os windows linux`는 게스트 워커를 각 VM 안에서 빌드합니다. 태그를 밀지 않고도 세 OS 종단 확인을 할 수 있는 경로입니다.

맥에 필요한 것: Git, Rust, 그리고 `prlctl exec`과 Parallels Tools가 동작하는 Parallels. 선택한 VM은 실행 중이고 데스크톱 사용자가 로그인해 있어야 합니다. VM 이름과 도구 버전은 [machine.json](machine.json)에 있습니다.

컨트롤러는 `windows`, `linux`, `macos`를 선택하며 `--os`를 생략하면 셋 모두를 선택합니다. 매트릭스 실행은 기본이 순차이고 `--execution parallel`을 지정하면 동시에 실행합니다. 병렬도 단계·명령·로그·재시도·산출물·실패 원인을 순차와 같은 필드에 기록합니다. 하나의 소스 스냅샷을 사용하고 모든 선택 플랫폼의 결과를 기다린 뒤 하나라도 실패하면 전체 명령도 실패합니다. `--result-file PATH`는 다른 인터페이스를 위한 구조화된 결과를 기록합니다. 진단/준비 결과와 시각은 `.state/tool-status.json`에 유지하며 머신 설정이 바뀌면 이전 결과를 표시하지 않습니다.

```sh
build-machine doctor --os linux
build-machine setup --os linux
build-machine build ~/Work/airdata --os linux --run
build-machine run ~/Work/airdata --os linux

# 저장소 workflow를 재현합니다. test나 smoke 게이트를 생략하려면
# 위에서 설명한 skip 주석이 필요합니다.
build-machine ci validate ~/Work/airdata --workflow .github/workflows/release-macos.yml
build-machine ci run ~/Work/airdata --workflow .github/workflows/release-macos.yml --ref v0.1.0 --execution parallel --os macos
```

`build`는 빠진 사전 요구사항을 자동으로 설치합니다. Linux 시스템 패키지는 Parallels를 통해 root로 설치하고, 빌드와 실행은 로그인된 데스크톱 사용자로 수행합니다. 실행 명령은 그 사용자의 systemd 환경에서 데스크톱 연결 설정만 읽습니다. 프로세스가 유지되는지 확인하며, 화면에 실제로 보이는지는 별도 화면 캡처로 확인합니다.

사용자 정의 프로젝트는 `build`에 `--framework custom --command '빌드 명령' --artifact '상대/경로'`를 지정합니다. 사용자 정의 명령은 해당 플랫폼의 셸에서 데스크톱 사용자로, 그 프로젝트의 스냅샷 안에서 실행됩니다.

전송에는 `WindowsBuildMachine`이라는 읽기 전용 Parallels 공유 폴더를 사용합니다. 게스트는 그 공유에서 워커 바이너리와 소스 스냅샷을 읽고, 그쪽으로 되쓰지 않습니다. 맥의 전송 파일·실행 기록·로그는 Git에서 제외한 `.state/`에 있습니다. 각 실행은 `.state/runs/<run>/report.json`에, 그 로그는 `.state/logs/<run>-*.log`에 기록하며 [machine.json](machine.json)의 `retention` 정책이 기간·프로젝트별 개수·전체 용량으로 둘을 함께 제한하고 오래된 실행부터 지웁니다. Windows의 프로젝트와 빌드 기록은 `C:\BuildMachine\projects`에 있습니다. MSVC는 `C:\BuildTools`, 사용자 도구는 `%LOCALAPPDATA%\WindowsBuildMachine`에 설치합니다.

MSVC는 Microsoft가 서비스하는 Visual Studio 2022 채널을 사용하고 필요한 구성 요소를 확인합니다. 다른 도구 버전과 다운로드 체크섬은 `machine.json`에 고정돼 있습니다. 구성 요소 요구사항을 만족하는 기존 MSVC 설치를 자동으로 업그레이드하지 않습니다. 이는 반복 가능한 프로비저닝이며 비트 단위로 재현 가능한 컴파일러 출력을 주장하는 것이 아닙니다.

## 검증

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask test
```

실제 결과는 [verification.ko.md](verification.ko.md)에 기록합니다. 아직 수행하지 않은 플랫폼·프레임워크 범위도 거기에 명시합니다.
