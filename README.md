# LifeOps

Rust 서버와 Svelte 프론트엔드로 구성된 개인용 LifeOps 애플리케이션입니다. macOS에서는 Tauri 데스크탑 앱으로 실행할 수 있으며, 앱이 실행되는 동안 같은 로컬 네트워크의 다른 기기에서도 브라우저로 접속할 수 있습니다.

## 데스크탑 앱(macOS) 빌드·설치

### 빌드

사전 요구 사항은 Rust 툴체인, Node.js/npm, 그리고 Tauri CLI 2입니다. 저장소 루트에서 다음 명령을 실행합니다.

```sh
cd frontend
npm install
npm run build
cd ../crates/lifeops-tauri
cargo tauri build
```

서명되지 않은 DMG 산출물은 워크스페이스의 `target/release/bundle/dmg/LifeOps_<버전>_*.dmg`에 생성됩니다.

### 설치와 첫 실행

이 빌드는 코드 서명과 공증을 하지 않은 개인용 앱입니다.

1. `.dmg`를 열고 `LifeOps.app`을 **Applications(응용 프로그램)** 폴더로 드래그합니다.
2. Finder에서 `LifeOps.app`을 우클릭하고 **열기**를 선택한 뒤 다시 **열기**를 누릅니다.
3. 우클릭 실행으로 열리지 않을 때만 터미널에서 격리 속성을 제거한 뒤 다시 실행합니다.

   ```sh
   xattr -d com.apple.quarantine /Applications/LifeOps.app
   ```

`xattr` 명령은 설치한 `LifeOps.app`에만 사용하세요. 출처를 신뢰할 수 없는 앱에는 적용하지 않는 것이 좋습니다.

### 데이터와 네트워크 접속

- 앱 데이터는 `~/Library/Application Support/LifeOps/`에 저장됩니다. 앱을 제거해도 이 폴더는 자동 삭제되지 않습니다.
- 같은 LAN의 다른 기기는 앱의 **설정** 화면에 표시된 `http://<IP>:<포트>` 주소로 접속합니다. 주소가 열리지 않으면 두 기기가 같은 네트워크인지와 macOS 방화벽의 LifeOps 인바운드 연결 허용 여부를 확인하세요.
- LAN 주소에는 인증이 적용되지 않으므로 신뢰할 수 있는 가정·개인 네트워크에서만 앱을 실행하세요. 공용 Wi-Fi나 인터넷에 포트를 노출하지 마세요.
- 메인 창을 닫아도 앱은 메뉴 막대(트레이)에 남아 서버와 LAN 접속을 유지합니다. 완전히 종료하려면 메뉴 막대의 LifeOps 메뉴에서 **종료**를 선택합니다.
- 자동 시작은 기본적으로 활성화됩니다. **설정** 화면에서 끄거나 다시 켤 수 있으며, 활성화되어 있으면 macOS 로그인 후 LifeOps가 자동 실행됩니다.

