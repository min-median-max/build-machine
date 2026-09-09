# 세 운영체제의 릴리즈 사전 검증

맥에서 세 개의 네이티브 실행 환경을 제어합니다. 현재 macOS 호스트, Parallels의 Windows VM, Parallels의 Linux VM입니다. macOS 위의 Linux 컨테이너를 Windows나 macOS 실행 검증으로 간주하지 않습니다.

선택한 플랫폼마다 빌드 도구를 진단하고, 정의된 도구가 없으면 설치하고, 해당 플랫폼의 실제 산출물을 빌드한 뒤 패키징된 앱을 확인합니다. 게시는 로컬 산출물 목록으로 모의 처리합니다. 릴리즈 업로드와 운영 서명은 별도의 실제 검증이 필요하며 로컬 모의 검증으로 완료했다고 기록하지 않습니다.

선택한 플랫폼들이 기록된 동일한 소스 스냅샷을 사용합니다. 소스 리비전, 작업 트리 수정 여부, 플랫폼, CPU 대상, 도구 버전, 명령, 결과, 산출물 체크섬을 남깁니다. 개발 빌드는 검증된 결과를 재사용할 수 있지만 릴리즈 사전 검증은 패키징과 실행 확인을 다시 수행해야 합니다. 의존성 캐시는 재사용할 수 있습니다.

## 플랫폼 연결

| 플랫폼 | 로컬 실행 | 대응할 GitHub Actions 러너 | 빌드 대상 |
| --- | --- | --- | --- |
| Windows | Parallels의 Windows 11 ARM64 | `windows-11-arm` | `aarch64-pc-windows-msvc` |
| Linux | Parallels의 Ubuntu 26.04 ARM64 | `ubuntu-26.04-arm` (공개 프리뷰) | `aarch64-unknown-linux-gnu` |
| macOS | 현재 ARM64 맥 | 기존 airdata 워크플로의 `macos-14` | `universal-apple-darwin` |

빌드 대상을 맞춰도 로컬 OS 이미지와 GitHub 호스팅 이미지 전체가 같아지는 것은 아닙니다. Windows x64와 Intel macOS에서의 네이티브 실행은 별도의 검증 대상이며 ARM64 실행으로 확인되지 않습니다. macOS 유니버설 산출물은 두 아키텍처를 포함할 수 있지만 로컬 실행 검증은 실제 실행한 아키텍처만 확인합니다.

## 현재 구현 상태

Windows와 Ubuntu의 도구 준비·doctor·AIRDATA 빌드·실제 창 표시를 구현하고 검증했습니다. 공통 제어 명령에는 네이티브 macOS 작업 스크립트도 포함됩니다. macOS 도구 준비는 통과했고 앱과 설치 패키지 검증은 남아 있습니다. Windows 설치 패키지 검사와 GitHub Actions의 공통 스크립트 연결은 진행 중입니다. Ubuntu 26.04 LTS ARM64와 Parallels Tools가 설치되어 있습니다. Linux 승인 기준은 의존성 자동 설치, AIRDATA 빌드 성공, 실제 앱 창 표시, 반복 명령에서 검증된 빌드와 프로세스 재사용입니다. 실제 증거는 [verification.md](verification.md)에 있습니다.

기존 airdata 워크플로는 Node.js 22, pnpm 11, stable Rust로 macOS 유니버설 앱을 빌드합니다. 릴리즈 액션이 GitHub에 게시하고 조건에 따라 서명·공증합니다. 초기 Windows 검증에서는 Node.js 24를 사용했고 설치 패키지 없이 실행 파일을 빌드했습니다. 그것만으로 기존 릴리즈 워크플로를 검증했다고 볼 수는 없습니다.

참고: [GitHub 호스팅 러너 이름과 아키텍처](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
