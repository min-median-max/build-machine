# 세 운영체제에서 빌드되는 프로젝트로 만들기

[English](ADOPTING.md) · [머신이 지원하는 범위](PROJECTS.ko.md)

이 머신이 당신의 저장소를 Windows·Ubuntu·macOS에서 빌드하고, 태그를 붙이기 전에
릴리즈를 리허설할 수 있게 하려면 무엇을 바꿔야 하는지 정리한 문서입니다.

이 머신에만 해당하는 요구는 없습니다. 세 운영체제로 출시한다고 말하는 프로젝트라면
어차피 답할 수 있어야 하는 것들입니다 — 어느 커밋을 빌드했는지, 테스트가 돌았는지,
앱이 실제로 뜨는지. 머신은 그 답이 없을 때 추측하기를 거부할 뿐입니다.

체크리스트를 따라간 뒤 `build-machine ci validate`를 실행하세요. 아직 충족하지 않은
항목을 정확히 알려줍니다.

---

## 1. 단위는 저장소입니다

- **Git 저장소 루트를 등록합니다.** 하위 디렉터리가 아닙니다. 모노레포의 하위 앱은
  workflow 단계의 `working-directory`로 선택합니다.
- **잠금 파일을 커밋하세요.** `pnpm-lock.yaml` 또는 `package-lock.json`, Tauri라면
  `src-tauri/Cargo.lock`. 여기서의 빌드는 `--frozen-lockfile`과 `--locked`입니다.
  잠금이 어긋나면 새로 해석하지 않고 실패합니다.
- **프로젝트 밖을 가리키는 심볼릭 링크와 서브모듈은 안 됩니다.** 스냅샷은 추적 파일과
  무시되지 않은 미추적 파일을, 커밋하지 않은 수정까지 포함해 복사합니다. 바깥으로
  나가는 것은 조용히 따라가지 않고 거부합니다.
- **기계에만 있는 비밀은 추적 파일에 두지 마세요.** 무시된 파일은 복사되지 않으며,
  그래서 빌드가 그런 파일에 의존해서도 안 됩니다.

## 2. 빌드는 사람 없이 끝나야 합니다

- 대화형 프롬프트, 개발 서버, 특정 개발자 기계의 절대 경로가 없어야 합니다.
- 프레임워크의 프로덕션 빌드를 쓰세요. Tauri라면 `tauri build`이고, 머신이
  `--ci --no-sign --locked`와 타깃을 붙입니다.
- 플랫폼당 **하나**의 식별 가능한 실행 파일을 만들거나 `--artifact`로 지목하세요.
  출력 디렉터리에 후보가 둘이면 임의로 고르지 않고 오류입니다.

## 3. 선언된 도구 버전에 맞추세요

머신은 [machine.json](machine.json)이 선언한 것만 설치하고 PATH 앞에 둡니다.
`.nvmrc`나 `rust-toolchain.toml`을 읽지 않습니다.

선언된 Node.js·pnpm·Rust·Go에서 프로젝트가 동작하게 하거나, `machine.json`을 바꾸고
그 이유를 남기세요. `build-machine doctor`가 실제 설치된 것과 선언된 것을 비교해
보고합니다.

## 4. 플랫폼마다 필요한 것을 감안하세요

| 플랫폼 | 프로젝트가 견뎌야 하는 것 |
| --- | --- |
| Windows | WebView2, MSVC, ARM64. `.cmd` 심은 직접 실행되지 않으므로 `cmd.exe`를 거칩니다. |
| Ubuntu | WebKitGTK와 `machine.json`의 `packages`에 있는 시스템 라이브러리. |
| macOS | 유니버설 빌드는 두 아키텍처를 모두 담아야 합니다. 머신이 `lipo`로 확인하고 하나가 없으면 실패합니다. |

의도한 OS용으로 컴파일하는 것은 당신 코드의 몫입니다. 머신은 선언된 사전 요구사항을
설치할 뿐, 소스에서 임의의 네이티브 라이브러리를 추론하지 않습니다.

## 5. Workflow가 계약입니다

`ci validate`와 `ci run`은 `.github/workflows/*.yml` 하나를 읽고, 이 머신이 실제로
수행할 수 있는 부분을 재현합니다. 그 밖의 것은 조용히 다른 것으로 바꾸지 않고 검증에서
실패합니다.

**지원하는 action** — `actions/checkout`, `pnpm/action-setup`,
`actions/setup-node`, `dtolnay/rust-toolchain`, `swatinem/rust-cache`,
`actions/cache`, `tauri-apps/tauri-action`, `actions/upload-artifact`,
`actions/upload-pages-artifact`, `softprops/action-gh-release`. 셸 `run` 단계는
쓰인 그대로 실행됩니다.

**지원하지 않는 것** — 컨테이너, 서비스, 재사용 workflow, `strategy.matrix`,
어댑터가 없는 action. 검증에서 실패합니다.

**표현식** — 로컬에서 값을 알 수 없는 조건은 실행을 실패시킵니다. 비밀에 의존하는
조건은 거짓으로 두고 제한으로 기록하므로, 서명 단계는 어중간하게 시도되지 않고
건너뜁니다.

### 세 개의 게이트

workflow에는 **build**가 있어야 하고, **test**와 **smoke**는 있거나 없는 이유를
밝혀야 합니다:

```yaml
# build-machine: skip smoke reason=데스크톱 실행은 로그인된 세션에서 따로 확인합니다
```

이유는 플랫폼 결과에 저장됩니다. 게이트의 요점이 이것입니다 — 빠진 검사가 아무도
눈치채지 못한 누락이 아니라 기록에 남은 결정이 됩니다.

건너뛰기보다 실제 단계를 두세요. 저장소에 이미 테스트가 있다면 그걸 실행하는 것으로
대개 끝납니다.

### 단계가 스테이지에 배정되는 방식

지원 action을 쓰는 단계는 그 어댑터가 배정합니다. checkout, pnpm/Node/Rust 준비와
캐시는 `setup`, Tauri action은 `build`, 산출물 업로드와 릴리스는 `release`입니다.
셸 `run` 단계만 자기 이름과 명령으로 읽으며, `smoke`/`launch`/`health`/`e2e`,
그다음 `test`/`lint`/`check`/`verify`, 그다음 `build`/`package`/`compile` 순서입니다.

그러니 셸 단계 이름은 하는 일대로 지으세요. action의 이름은 절대 세지 않습니다 —
`actions/checkout`에 "check"가 들어 있지만 test 스테이지가 되지 않습니다.

### 한 플랫폼을 못박은 workflow는 그곳에서만 재현됩니다

`runs-on`이 그 job이 어디서 돌도록 쓰였는지 말하고, `--target universal-apple-darwin`
같은 인자가 한 번 더 말합니다. 그런 workflow를 다른 곳에서 재현하는 것은 시작 전에
거부합니다:

```
이 워크플로는 macos 에서 실행되도록 작성됐어요. linux에서는 재현할 수 없습니다.
```

`ci validate`가 어느 플랫폼에서 재현 가능한지 보고하므로 실행하지 않고도 알 수 있습니다.

세 운영체제를 모두 리허설하려면 **타깃을 직접 적지 마세요.** 머신이 빌드하는 플랫폼의
타깃을 `machine.json`에서 가져와 붙입니다. 그 한 플랫폼만을 뜻할 때만 직접 적으세요.

### 재현이 증명하지 않는 것

서명, 공증, 산출물 업로드, 릴리스 게시는 로컬 어댑터로 대체하고 제한으로 기록합니다.
성공한 재현은 `passed_with_limits`이며, 실제 릴리스가 게시된다는 증거가 아닙니다.

---

## 체크리스트

```
[ ] 등록하는 것은 Git 저장소 루트
[ ] 잠금 파일이 커밋돼 있음
[ ] 외부 심볼릭 링크와 서브모듈 없음
[ ] 프롬프트와 개발자별 경로 없이 빌드가 끝남
[ ] 플랫폼당 식별 가능한 실행 파일 하나, 또는 --artifact로 지목
[ ] machine.json이 선언한 버전에서 프로젝트가 동작함
[ ] workflow가 지원 action만 사용, matrix·컨테이너 없음
[ ] build 스테이지가 있음
[ ] test 스테이지가 있거나, 없는 이유가 skip 주석에 있음
[ ] smoke 스테이지가 있거나, 없는 이유가 skip 주석에 있음
[ ] build-machine ci validate 통과
```

---

## 적용 예시

AIRDATA는 macOS 앱을 빌드해 게시하는 릴리즈 workflow를 가진 Tauri 2 앱입니다.
Rust 테스트 38개를 갖고 있었지만 workflow가 한 번도 돌리지 않았고, smoke 단계도
없었습니다. 검증이 거부했습니다:

```
ERROR: test 단계가 없어요. 워크플로에 '# build-machine: skip test reason=...' 주석을 추가해야 해요.
```

두 가지 변경으로 계약을 충족했습니다. 테스트는 이미 있었으므로 실행만 시키면 됐습니다:

```yaml
      - name: Test the frontend types and the Rust core
        run: |
          pnpm exec tsc --noEmit
          cargo test --locked --manifest-path src-tauri/Cargo.toml
```

그리고 호스티드 러너가 실제로 할 수 없는 단계에는 이유를 남겼습니다:

```yaml
# build-machine: skip smoke reason=the desktop launch is verified by `build-machine run` against a signed-in desktop session, which a hosted runner does not have
```

그러자 재현이 끝까지 돌았습니다:

```
전체: passed_with_limits
  setup    passed_with_limits   checkout · pnpm · node · rust · cache · install · 서명(건너뜀)
  test     passed               Rust 테스트 38개와 프런트엔드 타입 검사
  build    passed               tauri-action → data_0.1.0_universal.dmg
  smoke    passed_with_limits   skip smoke=…
```

같은 workflow를 Ubuntu에서 재현하자 macOS 전용 workflow로는 볼 수 없던 것이 나왔습니다.
테스트 38개 중 하나가 거기서 실패했습니다. 첫 클라이언트의 글이 반영되기를 기다리지
않고 두 번째 클라이언트를 붙여 `items[0]`을 읽고 있었고, 기계가 충분히 빠를 때만
통과하고 있었습니다. 같은 파일에 이미 기다리는 패턴이 있었는데 그 테스트만 쓰지
않았습니다. 고친 뒤 두 곳 모두에서 38개가 통과합니다.

이것이 세 OS 리허설의 요점이고, 동시에 이 workflow를 Ubuntu에서 재현하는 것을 이제
더 일찍 거부하는 이유이기도 합니다. `runs-on`과 `--target universal-apple-darwin`이
둘 다 macOS를 가리킵니다. 세 플랫폼을 위한 workflow라면 타깃을 직접 적어서는 안 됩니다.

이제 태그를 붙이기 전에 테스트가 돌아갑니다. 이 머신이 강요한 형식이 아니라 그
프로젝트 자신의 릴리즈 품질이 달라진 것입니다.
