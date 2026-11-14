## 설치

| 파일 | 대상 |
|---|---|
| `EXAONE.Studio_*_aarch64.dmg` | macOS (Apple Silicon) 앱 |
| `EXAONE.Studio_*_x64-setup.exe` | Windows 앱 설치 프로그램 |
| `EXAONE.Studio_*_x64_en-US.msi` | Windows 앱 (MSI) |
| `exaone-*-macos-arm64.tar.gz`, `exaone-*-windows-x64.zip` | `exaone` 명령줄 도구 (선택) |

앱만 설치하면 됩니다. 처음 켤 때 실행 환경(LiteLLM, 약 500MB)을 스스로 내려받아 설치합니다. Claude Code·Codex를 EXAONE으로 쓰려면 각 도구가 설치돼 있어야 합니다.

**서명되지 않은 앱입니다.** 처음 열 때 경고가 나옵니다.

- macOS: 앱을 응용 프로그램 폴더로 옮긴 뒤 우클릭 → 열기. 그래도 열리지 않으면 터미널에서 `xattr -dr com.apple.quarantine "/Applications/EXAONE Studio.app"`
- Windows: SmartScreen 창에서 "추가 정보" → "실행"
