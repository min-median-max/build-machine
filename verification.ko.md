# 검증 기록

[English](verification.md) · [GUI 검증](GUI-VERIFICATION.ko.md)

데스크톱 GUI, Windows Node.js 22.23.2 준비, 영구 환경 결과와 아직 해결되지 않은 Parallels 실행 오류는 [GUI-VERIFICATION.ko.md](GUI-VERIFICATION.ko.md)에 따로 기록합니다.

## Workflow 재현 구현

지원하는 GitHub Actions subset을 엄격하게 읽고 로컬에서 실행하는 workflow 재현을 추가했습니다. macOS 호스트에서 `python3 -m unittest discover -s tests -v`가 23개 테스트를 통과하며 지원하지 않는 action·게이트 누락의 명시적 실패, 변경할 수 없는 ref 스냅샷, 저장소 루트 검증, 순차·병렬 보고서 동일성, 네이티브 workflow 단계 실행을 포함합니다. `~/.cargo/bin/cargo test --manifest-path gui/src-tauri/Cargo.toml`은 Rust 테스트 10개를 통과했고 설정된 Ubuntu doctor 테스트 1개는 계속 무시합니다. `pnpm --dir gui test`는 브라우저 테스트 13개, `pnpm --dir gui run build`는 TypeScript와 Vite 빌드를 통과했습니다.

현재 유지하는 AIRDATA workflow에는 test나 smoke 단계와 `build-machine: skip ... reason=...` 주석이 없습니다. 따라서 `ci validate`는 필요한 게이트 누락 메시지로 거부하며 저장소가 생략 사유를 문서화한 뒤에 재현할 수 있습니다. 이 검사에서는 AIRDATA 소스·GitHub workflow·서명 자격 증명·업로드·릴리즈를 변경하거나 실행하지 않았습니다. Windows와 Ubuntu 재현 코드는 통제된 테스트로 검증했으며 실제 재현은 설정한 게스트에서 별도 실행해야 합니다.
