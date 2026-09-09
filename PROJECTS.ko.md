# 프로젝트 빌드 조건

[English](PROJECTS.md)

현재 빌드 머신이 프로젝트에 요구하는 조건입니다. Tauri와 Wails 자체가 공통 빌드 머신 설정 파일을 요구하는 것은 아닙니다. 이 저장소는 아직 그런 파일을 읽거나 `doctor`로 프로젝트 규약 전체를 검사하지 않습니다.

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

현재 `doctor`는 머신의 필수 도구와 선언된 버전을 검사합니다. 프로젝트 파일 전체, 프레임워크 호환성, 패키징 조건, 기능 동작까지 검사하지는 않습니다. 프로젝트 규약 전체의 진단과 프로젝트별 재사용 가능한 릴리즈 정의는 아직 구현하지 않았습니다.

이 GUI에서는 `python3 desktop.py dev`로 React 개발 서버와 Rust/Tauri 프로세스를 함께 실행합니다. 다른 프로젝트는 자체 Tauri 또는 Wails 개발 명령과 설정을 사용합니다. 빌드 머신이 화면의 포트 번호를 고정하도록 요구하지는 않습니다.

참고: [Tauri 필수 도구](https://v2.tauri.app/start/prerequisites/), [Tauri 개발](https://v2.tauri.app/develop/), [Wails 2 설치](https://wails.io/docs/gettingstarted/installation/), [Wails 3 문서](https://v3.wails.io/).
