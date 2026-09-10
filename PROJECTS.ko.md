# 프로젝트 빌드 조건

[English](PROJECTS.md)

현재 빌드 머신이 프로젝트에 요구하는 조건입니다. 프로젝트는 Git 저장소 루트에서만 등록합니다. Tauri와 Wails 자체가 별도 빌드 머신 manifest를 요구하지 않으며 workflow 재현을 선택하면 저장소 workflow가 CI 계약이 됩니다.

## 현재 자동 빌드 절차

| 프로젝트 | 필요한 파일과 동작 | 현재 검증 범위 |
| --- | --- | --- |
| Tauri 2 | `src-tauri/tauri.conf.json`, `src-tauri/Cargo.lock`, 루트의 `pnpm-lock.yaml` 또는 `package-lock.json`, 프로젝트의 Tauri CLI 의존성이 필요합니다. 설정한 화면 빌드가 사용자 입력 없이 실행되어야 합니다. | AIRDATA로 Windows ARM64 실행 파일과 Ubuntu ARM64 실행 파일/DEB를 검증했습니다. macOS 유니버설 작업 스크립트는 구현했지만 AIRDATA 검증은 남아 있습니다. |
| Wails 2 | 자동 감지를 위한 루트 `wails.json`과 `go.mod`의 버전이 명시된 `github.com/wailsapp/wails/v2` 의존성이 필요합니다. 화면 빌드 절차는 프로젝트가 정의합니다. | Windows 절차는 지정된 Wails CLI를 설치하고 `windows/arm64`를 빌드합니다. 실제 프로젝트 검증은 아직 하지 않았습니다. Linux/macOS 자동 절차는 없습니다. |
| Wails 3 beta | 자동 감지와 빌드 절차가 아직 없습니다. | CLI에서 프로젝트별 명령을 직접 지정해야 합니다. 검증하지 않았습니다. |
| 그 밖의 구조 | 명시적인 빌드 명령과 소스 스냅샷 기준 실행 파일 상대 경로가 필요합니다. | CLI 사용자 지정 절차가 있습니다. 실제 프로젝트로 검증하지 않았습니다. 사용자 지정 macOS 앱 실행은 구현하지 않았습니다. |

GUI는 현재 자동 감지를 사용합니다. 사용자 지정 명령과 산출물 경로는 CLI에서 지정합니다.

```sh
python3 build.py build /path/to/project --os linux \
  --framework custom --command './scripts/build-linux.sh' \
  --artifact 'build/bin/example'
```

사용자 지정 명령은 Windows의 `cmd.exe`, Linux/macOS의 `/bin/sh`에서 실행합니다. 셸이나 출력 경로가 다르면 플랫폼별 명령을 따로 지정합니다.

## 프로젝트 공통 조건

- 기존 커밋이 있는 Git 체크아웃을 사용하고 빌드 스크립트와 의존성 잠금 파일을 Git에 넣습니다. 스냅샷은 현재 수정과 무시되지 않은 미추적 파일도 포함합니다. Git에서 무시한 로컬 의존성과 비밀정보는 복사하지 않습니다. 외부 심볼릭 링크와 Git 서브모듈은 지원하지 않습니다.
- 개발자 개인의 절대 경로, 대화형 입력, 로컬 개발 서버 없이 빌드되어야 합니다. 프레임워크의 배포용 화면 빌드를 사용하고 프로젝트 파일은 체크아웃 기준으로 찾습니다.
- 정의한 컴파일러와 런타임 버전이 프로젝트와 호환되어야 합니다. 현재 도구는 머신의 `machine.json`에 고정하며 프로젝트의 버전 파일마다 따로 선택하지 않습니다. Windows/Linux에는 ARM64 네이티브 의존성이 필요하고 macOS 작업 스크립트는 ARM64와 Intel 코드를 포함한 유니버설 앱을 요청합니다.
- 운영체제별 코드를 대상 OS에 맞게 컴파일합니다. Windows WebView2와 Linux WebKit/시스템 라이브러리를 고려해야 합니다. 머신은 정의한 필수 도구를 설치하며 소스에서 임의의 네이티브 라이브러리를 추론하지 않습니다.
- 실행 파일 후보가 하나가 되도록 하거나 `--artifact`로 지정합니다. Tauri macOS 빌드는 `.app` 하나를 만들어야 합니다. 컴파일 성공, 패키지 생성, 패키지 설치, 실제 창 표시, 앱 기능 검사는 별도 결과입니다.
- 릴리즈 서명과 게시 설정을 로컬 검증과 구분합니다. 현재 작업 스크립트는 운영 서명·공증·릴리즈 게시를 검증하지 않습니다. 서명하지 않은 로컬 빌드 성공으로 GitHub Actions 릴리즈와의 동등성이 확인되지는 않습니다.

현재 `doctor`는 머신의 필수 도구와 선언된 버전을 검사합니다. `ci validate`는 선택한 workflow의 프로젝트 계약을 검사하지만 앱 기능, 운영 서명, 공증이나 GitHub 게시를 보증하지 않습니다. 외부 서비스가 미검증인 workflow는 `passed_with_limits` 상태로 명시할 수 있습니다.

## Workflow 재현 규약

`ci validate`와 `ci run`은 하나의 `.github/workflows/*.yml` 파일을 읽습니다. `runs-on`, `needs`, `if`, `env`, `with`, `working-directory`, `run`과 아래에 정의한 action 어댑터를 지원합니다. 컨테이너, 서비스, 재사용 workflow, matrix 전략이 있는 job은 조용히 바꾸지 않고 검증에서 실패합니다.

지원 action 어댑터는 `actions/checkout`, `pnpm/action-setup`, `actions/setup-node`, `dtolnay/rust-toolchain`, `swatinem/rust-cache`, `actions/cache`, `tauri-apps/tauri-action`, `actions/upload-artifact`, `actions/upload-pages-artifact`, `softprops/action-gh-release`입니다. `run` 단계는 선택한 작업자에서 실행합니다. checkout·캐시·산출물 업로드·릴리즈·서명·GitHub 상태 작업은 로컬 어댑터를 사용하며 보고서에 제한을 기록하고 실제 게시로 취급하지 않습니다.

지원하는 action을 사용하는 단계는 어댑터가 스테이지를 결정합니다. checkout, pnpm/Node/Rust 준비와 캐시는 `setup`, Tauri action은 `build`, 산출물 업로드와 릴리스는 `release`입니다. 이름과 명령의 키워드로 분류하는 대상은 셸 `run` 단계뿐이므로, action의 이름이 스테이지 게이트를 대신 충족시키는 일은 없습니다. 기본 단계 순서는 `setup`, `test`, `build`, `smoke`, `release`입니다. test나 smoke 명령이 없는 workflow는 `# build-machine: skip smoke reason=데스크톱 smoke는 별도 검증`처럼 정확한 주석을 포함해야 합니다. 그 사유는 플랫폼 결과에 기록합니다. 알 수 없는 action, 지원하지 않는 표현식, 이벤트/ref 불일치와 게이트 누락은 검증에서 실패합니다.

소스는 현재 Git 작업 트리(무시되지 않은 수정 포함) 또는 변경할 수 없는 `--ref` archive 중 하나입니다. matrix 각 항목은 같은 소스 리비전·소스 hash·변경 상태를 기록합니다. 순차 실행이 기본이며 `--execution parallel`은 선택한 OS를 동시에 실행하고 동일한 단계 필드와 대시보드 순서를 유지합니다.

이 GUI에서는 `python3 desktop.py dev`로 React 개발 서버와 Rust/Tauri 프로세스를 함께 실행합니다. 다른 프로젝트는 자체 Tauri 또는 Wails 개발 명령과 설정을 사용합니다. 빌드 머신이 화면의 포트 번호를 고정하도록 요구하지는 않습니다.

참고: [Tauri 필수 도구](https://v2.tauri.app/start/prerequisites/), [Tauri 개발](https://v2.tauri.app/develop/), [Wails 2 설치](https://wails.io/docs/gettingstarted/installation/), [Wails 3 문서](https://v3.wails.io/).
