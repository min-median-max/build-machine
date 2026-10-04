# 기능 변경

## 미출시

workflow 재현이 runner처럼 workflow를 실행하므로, 저장소의 CI를 push하기 전에 여기서 확인할 수 있습니다.

- `actions/setup-node`, `actions/setup-go`, `shivammathur/setup-php`가 단계에 직접 또는 버전 파일로 선언한 릴리스를 설치하고 이후 단계의 PATH 앞에 둡니다. Node.js·Go 아카이브와 Composer는 nodejs.org, go.dev, getcomposer.org가 공개한 체크섬과 대조하고, 릴리스마다 자기 디렉터리를 두므로 두 번째 재현은 아무것도 설치하지 않습니다. PHP는 아래에 적은 대로 hosted runner의 setup-php처럼 설치하며, 그 릴리스를 PATH의 `php`로 정합니다. 이전에는 `setup-node`가 workflow의 선언과 상관없이 `machine.json`의 Node.js로 대신했습니다.
- action 이름을 GitHub처럼 대소문자 구분 없이 맞춥니다. `Swatinem/rust-cache`가 검증에서 실패했습니다.
- 단계를 workflow 순서대로 실행합니다. 이전에는 스테이지별로 실행해서, `setup`으로 분류된 단계가 자신이 읽는 출력을 만드는 앞의 `test` 단계보다 먼저 실행됐습니다. 스테이지는 이제 보고 방식일 뿐입니다.
- 단계가 실패해도 runner처럼 계속합니다. 실패 뒤에는 `if:`가 허용하는 단계만 실행하고, 실행하지 않은 단계는 이유와 함께 `skipped`로 기록합니다. 이전에는 첫 실패에서 멈췄고 `if:`는 리터럴 값만 받았습니다. 이제 `success()`, `failure()`, `always()`, `cancelled()`, `!`, `&&`, `||`, `${{ }}`를 읽고, 그 밖의 context는 실행 중이 아니라 검증에서 실패합니다.
- `$GITHUB_ENV`와 `$GITHUB_PATH`가 단계의 변수와 PATH 항목을 이후 단계로 넘기고, `GITHUB_WORKSPACE`, `RUNNER_TEMP`, `RUNNER_OS`, `RUNNER_ARCH`를 설정합니다.
- `run:` 블록은 Linux와 macOS에서 `/bin/sh -eu`가 아니라 runner처럼 `bash -e`로 실행합니다. 재현은 workflow의 `rust-toolchain.toml`을 덮어쓰던 `RUSTUP_TOOLCHAIN`을 더 이상 설정하지 않습니다.
- build 단계가 없는 workflow는 test·smoke처럼 `# build-machine: skip build reason=...`로 이유를 밝힐 수 있습니다.
- 단계 키 `shell`, `continue-on-error`와 어댑터가 반영하지 않는 setup action 입력은 버려지지 않고 검증에서 실패합니다.
- `actions/checkout`이 `machine.json`의 `repositories`가 로컬 clone에 대응시킨 다른 저장소에 대해 `repository`, `ref`, `path`를 받습니다. 재현은 그 clone의 commit된 branch, tag 또는 commit을 `path`에 checkout합니다. 이 입력들은 검증에서 실패했으며, soksak 구성 요소 릴리스는 이 방식으로 `soksak-app/core`를 checkout합니다.
- 표현식이 재현의 event, revision, checkout한 branch나 tag에서 `github.event_name`, `github.sha`, `github.ref`, `github.ref_name`을 `run`, `env`, `with`, `working-directory`, `if`에서 읽습니다. 이 값들은 검증에서 실패했으며, soksak 릴리스는 `${{ github.ref_name }}`에서 `soksak-app/core`를 checkout합니다. branch나 tag 이름이 없는 commit의 재현은 `github.ref`나 `github.ref_name`을 읽는 workflow를 거부합니다.
- `repository` 없는 `actions/checkout`이 `path`를 받아 job 자신의 저장소를 커밋하지 않은 변경을 stage한 채로 workspace 아래 그 폴더에 checkout합니다. 이 입력은 검증에서 실패했으며, core와 registry의 CI는 자신의 저장소를 다른 저장소 옆의 `core/`와 `registry/`에 checkout합니다.
- README에 적힌 대로 `cargo xtask`가 동작합니다. 워크스페이스에 `xtask` alias가 없어서 `cargo xtask worker --os linux`, `cargo xtask test`, `cargo xtask build --run`이 "no such command"로 실패했습니다.
- 개발 빌드는 자신을 컴파일한 워크스페이스를 사용합니다. `--root` 없이 실행한 `target/debug/build-machine`은 자기 위치에서 위로 찾다가 Tauri가 `target/debug`에 복사한 `machine.json`에서 멈췄고, `ci run`이 "The named build-machine share already belongs to another directory"로 실패했습니다. 워크스페이스 밖의 실행 파일은 이제 기본 root가 없고 `--root`를 요구합니다. 이전에는 명령줄이 현재 디렉터리로 대신했습니다.
- 시간 제한은 workflow의 것입니다. job은 자신의 `timeout-minutes` 또는 GitHub의 360분 동안, 단계는 job에 남은 시간 안에서 자신의 `timeout-minutes`만큼 실행합니다. 이전에는 어떤 workflow도 선언하지 않은 스테이지별 고정 제한(test 단계 30분)으로 단계를 끊었습니다. 자신의 제한을 넘은 단계는 실패하고, 제한을 넘은 job은 취소되어 `cancelled()`가 참이 되며 그 뒤로는 취소 뒤에도 실행하는 단계만 실행합니다.
- 재현이 구현하지 않은 job 키와 workflow 키는 검증에서 실패합니다. job의 `if`, `continue-on-error`, `defaults`, `outputs`, `environment`와 workflow `defaults`는 무시됐습니다. workflow `env`도 무시됐고 이제 모든 단계에 전달됩니다. job마다 `$GITHUB_ENV`, `$GITHUB_PATH`, 상태를 따로 두고, job은 `needs`의 job이 성공했을 때만 실행하며, 다른 운영체제의 job을 기다리는 job은 검증에서 실패합니다.
- 단계는 프로세스가 끝나면 끝납니다. 단계의 출력을 잡고 있는 백그라운드 프로세스(`make test-servers`가 남기는 로그 reader)가 끝날 때까지 단계가 끝나지 않았습니다. 이제 GitHub runner처럼 출력을 5초 더 읽고, 그 프로세스는 그대로 둡니다.
- 단계 출력이 GitHub처럼 동작합니다. `id`가 있는 `run` 단계가 `$GITHUB_OUTPUT`에 쓰고, 같은 job의 이후 단계가 `if:`, `run:`, `env`, `working-directory`, 어댑터가 읽는 `with` 입력에서 `${{ steps.<id>.outputs.<name> }}`을 읽습니다. `==`, `!=`, 문자열·숫자 리터럴과 GitHub의 `&&`·`||` 값 규칙도 읽습니다. 검증은 참조를 확인하고, 값은 단계를 실행할 때 채웁니다. orm의 `setup-php` 단계는 검증이 받아들일 수 없던 `php-min` 단계의 출력에서 버전을 받습니다. 로컬 값이 없는 `env` 표현식은 실행 중에 빈 값으로 바뀌었는데, 이제 `run:`이나 `working-directory`의 표현식처럼 검증에서 실패합니다.
- `actions/checkout`이 runner처럼 Git 저장소를 만듭니다. 스냅샷에 `.git`이 없어서 `git log`, `git ls-files`, `git grep`을 읽는 orm 검사가 실패했습니다. 이제 컨트롤러가 저장소의 branch, tag, `HEAD`를 Git bundle로 archive 옆에 보내고, checkout은 로컬 mirror를 거쳐 `actions/checkout`의 명령으로 가져옵니다. 기본은 commit 하나, `fetch-depth: 0`은 모든 branch와 tag이며, 재현하는 branch나 tag를 checkout합니다. 작업 트리를 재현하면 커밋하지 않은 수정·삭제·추적하지 않는 파일을 재현하는 commit 위에 stage합니다. 이 두 가지와 기록이 GitHub remote가 아니라 로컬 저장소의 것이라는 점은 제한으로 기록합니다. 다른 것을 checkout하게 하는 입력(`ref`, `path`, `submodules` 등)은 무시되지 않고 검증에서 실패합니다.
- 모든 job은 `<work>/<저장소>/<저장소>`의 빈 `GITHUB_WORKSPACE`에서 시작합니다. 이전에는 같은 소스의 이전 재현이 풀어 둔 소스를 그 재현이 남긴 것과 함께 다시 썼고, checkout 전 단계도 runner에는 아직 없는 파일을 봤습니다. `GITHUB_SHA`, `GITHUB_REF`, `GITHUB_REF_NAME`, `GITHUB_REF_TYPE`, `GITHUB_EVENT_NAME`, `GITHUB_JOB`을 설정합니다. Linux에서는 job이 끝나면 runner처럼 그 job의 `RUNNER_TRACKING_ID`를 가진 프로세스를 종료하고, 다른 플랫폼에서는 제한으로 기록합니다. 머신 자체가 새 이미지가 아니라는 점은 모든 재현의 제한으로 기록합니다.
- `shivammathur/setup-php`가 hosted Ubuntu runner에서 setup-php가 가져오는 곳에서 PHP를 가져옵니다. 머신에 이미 있는 릴리스는 그것으로 전환하고, 그 밖의 릴리스는 머신의 Ubuntu 버전과 아키텍처용 setup-php 캐시 빌드(shivammathur/php-ubuntu)를 GitHub가 그 asset에 공개한 sha256과 대조한 뒤 setup-php 설치 스크립트처럼 설치합니다. 이전에는 Ubuntu 버전마다 PHP 릴리스가 하나뿐인 배포판 apt 저장소에서 설치해서, Ubuntu 26.04에서는 8.5만 있어 orm의 최저 릴리스 검사가 8.4를 얻을 수 없었습니다. 머신용 빌드가 없는 릴리스나 빌드에 없는 선언 확장은 그 이유와 함께 단계를 실패시킵니다. 버전이 앞 단계의 출력에서 올 수 있으므로 어댑터는 이제 runner처럼 단계가 실행될 때 비밀번호 없는 `sudo`로 실행하며, 재현 전에 PHP를 설치하던 권한 단계 `ci-system`은 제거했습니다.
- Ubuntu runner 라벨이 머신과 다른 Ubuntu 릴리스나 아키텍처를 가리키는 job(Ubuntu 26.04의 `ubuntu-24.04-arm`, `ubuntu-latest`, ARM64의 x64 라벨)은 그 차이를 제한으로 기록합니다. runner처럼 모든 단계에 `$GITHUB_STEP_SUMMARY` 파일이 있습니다. 거기에 덧붙이는 단계는 설정되지 않은 변수 때문에 실패했습니다.
- Linux 프로필이 자신이 대신하는 runner 이미지 `ubuntu-26.04-arm`(이미지 20260927.135.1)를 선언합니다. 공개된 toolset의 apt 패키지와 이미지 설치 스크립트가 설치하는 PHP 8.5 패키지이며, `setup`이 설치하고 `doctor`가 확인합니다. 머신에 이미지의 PHP 패키지가 들여오는 라이브러리가 없어서 setup-php의 PHP 8.4 캐시 빌드가 "libsodium.so.23: cannot open shared object file"로 실패했습니다. 이미지에는 있고 머신은 제공하지 않는 것(컴파일러, tool cache, Docker, 브라우저, 데이터베이스 서비스, 이미지의 PHP 설정)은 `machine.json`에 적고 그 runner의 job을 재현할 때마다 제한으로 기록합니다.
- setup-php가 PHP 릴리스를 `php`로 정하기 전에 실행되는지(`php<v> -v`와 선언한 확장) 확인합니다. 실행되지 않는 릴리스는 `ldd`가 보고한 없는 라이브러리를 밝혀 단계를 실패시키고, alternatives를 원래대로 되돌립니다. 이전에는 시작하지 못하는 8.4 빌드로 `php`를 바꿔서 그 뒤 머신의 모든 `php` 호출이 실패했습니다.
- runner 이미지의 패키지를 actions/runner-images처럼 `apt-get install --no-install-recommends`와 phased update 포함으로 설치합니다. 이전에는 apt의 추천 패키지까지 설치해서 이미지에 없는 패키지가 들어왔습니다(Linux 머신에서 `debhelper`부터 `ssh-import-id`까지 22개). 머신 자체의 패키지는 apt 기본값을 유지합니다.

## 0.1.0

Rust 워크스페이스 하나로 다시 작성했습니다. Python과 PowerShell을 제거했고 스크립트는 남기지 않았습니다.

- 컨트롤러가 라이브러리입니다. 명령줄과 데스크톱 앱은 그 위의 두 프런트엔드이고, 작업은 별도 프로세스와 결과 파일을 거치지 않고 앱 자신의 프로세스에서 실행됩니다. `--result-file` 핸드셰이크와 stdout 파싱, "결과 파일이 없어서 성공을 확인할 수 없어요" 경로가 사라졌습니다.
- 실행 리포트·머신 정의·소스 스냅샷·작업 요청을 한 번만 정의해 공유합니다. 데스크톱 브리지는 무타입 문서를 인덱싱하지 않고 그 타입으로 역직렬화하므로, 필드 이름이 바뀌면 대시보드가 조용히 비는 대신 컴파일 오류가 납니다.
- 각 운영체제는 미리 빌드한 워커 바이너리를 실행합니다. 게스트에는 Python도 PowerShell도 Rust 툴체인도 필요 없습니다. `.github/workflows/release.yml`이 각 워커를 자기 러너에서 네이티브로 빌드하고(크로스 링크 없음) 그 위에 macOS 앱을 조립합니다.
- 워크플로 판독은 YAML 파서와 타입 역직렬화를 씁니다. 지원하지 않는 구성에 대해 실패로 닫는 성질이 손수 만든 파서가 아니라 타입의 성질이 됐습니다.

### 이번 재작성으로 달라진 동작

두 워커가 서로 벌어져 있었습니다. 단일 구현이 각 차이를 정리합니다:

- 워크플로 재현은 워커 자신의 출력으로 리포트를 돌려줍니다. Windows 워커는 읽기 전용으로 마운트된 공유에 그 리포트를 쓰려 했으므로, Windows 재현은 완료될 수 없었습니다.
- 상태 규칙이 하나입니다. Windows는 재현에서 그냥 `passed`를 보고할 수 있었지만 Unix 워커는 그럴 수 없었습니다.
- 재현은 모든 플랫폼에서 산출물을 체크섬과 함께 기록합니다. Windows는 목록을 비워 두고 있었습니다.
- 단계 시간 초과가 모든 플랫폼에 적용되고, 시간 초과는 프로세스 그룹 전체를 종료하므로 셸의 자식이 파이프를 붙잡아 이를 무력화하지 못합니다.
- 체크섬은 소문자이고, 기록하는 문서에는 BOM이 없습니다. 모든 플랫폼에서 같습니다.
- 새 아카이브 작성기로 소스 스냅샷 해시가 바뀌므로, 이 버전 이후 첫 빌드는 이전 결과를 재사용하지 않고 다시 빌드합니다.
- 이전 `.state` 배치에서 옮겨오는 마이그레이션은 없으며, `winbuild.py`의 별도 Windows 진입점도 복원하지 않습니다. 명령은 `build-machine` 하나입니다.

## 0.0.1

### 수정

- Windows에서 `setup`이 매트릭스 전체를 중단시키지 않습니다. Windows 분기가 이 명령에서 플랫폼 결과를 대입하지 않아 `UnboundLocalError`가 모든 처리기를 지나쳐 올라왔습니다. `--os` 기본값이 세 환경이고 Windows가 먼저 실행되므로 `python3 build.py setup`과 데스크톱 앱의 공통 도구 준비가 통째로 실패했습니다. 이제 한 플랫폼의 예상하지 못한 결함은 예외 종류와 함께 그 플랫폼의 실패로 기록하며, 보고서 없이 실행이 끝나지 않습니다.
- 실행 기록 보존이 용량을 넘겼을 때 가장 최근 실행부터 지웠고, 실제 용량을 차지하는 로그는 재지도 제한하지도 않았습니다. 이제 보고서와 로그를 한 단위로 보존·삭제하며, 기간·프로젝트별 개수·전체 용량을 각각 적용하고 오래된 실행부터 지웁니다. 중단된 `running` 보고서는 기간 정책으로 회수합니다. 이전에는 영원히 남아 그 프로젝트의 마지막 실제 결과를 계속 가렸습니다. `winbuild.py` 진입점의 로그처럼 어떤 실행 기록도 가리키지 않는 로그는 기간으로 제한하며, 데스크톱 앱이 요청하는 `--result-file` 사본도 함께 정리합니다.
- 실행 기록을 `.state/runs/<run>/report.json` 한 곳에만 기록합니다. 대시보드가 읽던 `.state/<run>-result.json`은 어떤 정책도 정리하지 않았고, 보존 정책은 아무도 읽지 않는 다른 사본을 대상으로 했습니다. 이전 보고서는 다음 제어 명령 실행 때 기록 시각을 유지한 채 `.state/runs/` 아래로 옮깁니다. 읽는 곳이 없던 `manifest.json` 사본은 제거했습니다.
- `actions/checkout`이 test 게이트를 대신 충족시키지 않습니다. 지원 action을 쓰는 단계는 어댑터로 분류하고 키워드 분류는 셸 `run` 단계에만 적용합니다. action 이름에 `check`가 들어 있어, 테스트 명령이 없는 워크플로가 필수인 `# build-machine: skip test reason=...` 주석 없이 검증을 통과했습니다. **여기에 기대던 워크플로는 이제 그 주석이나 실제 테스트 단계를 추가하기 전까지 검증에 실패합니다.**
- 워크플로 재현 결과가 대시보드에 표시됩니다. workflow 경로가 있는 프로젝트는 `ci run`으로 실행되는데, 표시한다고 문서화해 둔 기록에서 그 결과가 전부 걸러지고 있었습니다. 재현 기록에는 표식을 붙이고, 파싱된 워크플로와 단계 정의는 대시보드로 넘기지 않습니다.
- 실행 기록이 가리키는 작업 로그가 더 이상 비어 있지 않습니다. 실패했을 때만 기록해서, 성공한 실행에서는 대시보드의 로그 버튼이 존재하지 않는 파일을 열었습니다. 이제 실행 내용, 플랫폼별 결과, 그 플랫폼의 명령 출력을 기록한 위치를 남깁니다.
- 워크플로 단계의 출력을 단계가 끝날 때까지 모아 두지 않고 그때그때 내보냅니다. `run` 단계와 Tauri action 모두 해당하며, 재현 중에도 데스크톱 앱에 도달합니다. 단계 시간 초과는 프로세스 그룹 전체를 종료하므로 셸의 자식 프로세스가 파이프를 붙잡아 시간 초과를 무력화하지 못합니다.
- 각 스테이지의 단계별 명령·종료 코드·출력을 대시보드에서 플랫폼별로 볼 수 있습니다. 실행기는 이미 기록하고 있었고 표시하는 화면만 없었습니다.

### 추가

- 지원하는 `uses`와 `run` 단계를 대상으로 GitHub Actions workflow 검증과 로컬 재현을 추가했습니다. 이벤트/ref, 소스 리비전과 변경 상태, setup/test/build/smoke/release 단계, 산출물 체크섬, 서명 제한과 외부 서비스 로컬 어댑터를 기록합니다.
- 변경할 수 없는 ref 스냅샷, Git 저장소 루트 등록, 기본 순차 matrix 실행과 선택적인 병렬 실행을 추가했습니다. 순차·병렬은 같은 플랫폼 결과 필드를 사용하며 보존 실행 기록은 머신 보존 정책으로 제한합니다.
- 데스크톱 앱에 프로젝트별 workflow 경로·이벤트/ref·matrix 실행 방식을 추가하고 대시보드에 재현 메타데이터와 제한 결과를 표시합니다.

- 등록 프로젝트의 빌드 요약, 최근 로그와 현재 GUI 작업 진행을 보여주는 대시보드를 추가했습니다. 중간 보고서와 소스 준비 실패를 보존해 미완료/실패 빌드에 이전 성공 표시가 남지 않게 했습니다.

- 공통 환경 진단/준비를 하단 설정 버튼 안에 배치했습니다. 등록된 프로젝트를 직접 선택해 해당 빌드·실행 컨트롤을 엽니다. `+` 등록, 중복 폴더의 기존 항목 선택, 등록 해제, 프로젝트별 빌드 대상/실행 옵션 보존을 추가했습니다. 기존 선택 프로젝트를 보존하고 설정 폴더를 열 수 있도록 했습니다.

- 유지보수하는 빌드 명령을 위한 macOS Tauri 2 GUI를 추가했습니다. 플랫폼 선택, 실시간 출력, 네이티브 폴더 선택을 제공합니다.
- 환경별 마지막 진단/준비 결과와 완료 시각을 앱 재시작 후에도 유지합니다. VM 연결 상태와 빌드 결과는 과거 도구 검사 결과와 구분합니다.
- 필요한 Windows 도구 버전과 실제 설치 버전을 포함한 명령 실패 원인을 UI와 로그에 보존합니다. CLI의 여러 플랫폼 선택과 구조화된 결과 파일을 지원합니다.
- 현재 프로젝트 빌드 조건과 아직 검증하지 않은 프레임워크/릴리즈 범위를 문서화했습니다.
