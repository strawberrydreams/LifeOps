# LifeOps

Rust 서버와 Svelte 프론트엔드로 구성된 개인용 LifeOps 애플리케이션입니다. macOS에서는 Tauri 데스크탑 앱으로 실행할 수 있으며, 필요할 때 설정에서 같은 로컬 네트워크의 다른 기기 접속을 허용할 수 있습니다.

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
- 메인 창을 닫아도 앱은 메뉴 막대(트레이)에 남아 서버와 설정된 접속 범위를 유지합니다. 완전히 종료하려면 메뉴 막대의 LifeOps 메뉴에서 **종료**를 선택합니다.
- 자동 시작은 기본적으로 활성화됩니다. **설정** 화면에서 끄거나 다시 켤 수 있으며, 활성화되어 있으면 macOS 로그인 후 LifeOps가 자동 실행됩니다.

## 접속 범위와 외부 접속

- 기본은 **내 기기에서만**(`127.0.0.1`) 접속됩니다. 설정 화면의 **접속 범위 → 같은 네트워크(LAN) 허용**을 켜면 같은 네트워크의 다른 기기가 표시된 LAN 주소로 접속할 수 있습니다. 변경은 **앱 재시작 후** 적용됩니다.
- LAN 주소가 열리지 않으면 두 기기가 같은 네트워크인지와 macOS 방화벽의 LifeOps 인바운드 연결 허용 여부를 확인하세요.
- LAN 허용은 인증이 없으므로 **신뢰하는 네트워크(집 등)에서만** 켜세요.
- 집 밖에서 접속하려면 **Tailscale 같은 개인 VPN**을 권장합니다. 공유기 포트포워딩은 서비스가 인증 없이 인터넷 전체에 노출되므로 권장하지 않습니다.

## 백업과 복원

- 설정 화면에서 **지금 백업**을 누르면 데이터베이스·스키마·뷰·카테고리가 하나의 `.zip` 스냅샷으로 저장됩니다. 하루 1회 자동 백업도 동작합니다.
- **백업 폴더**를 iCloud Drive·Dropbox 같은 동기화 폴더로 지정하면 운영체제의 동기화를 통해 오프사이트 백업을 보관할 수 있습니다.
- 목록에서 **이 시점으로 복원**을 누르면 재시작 후 해당 스냅샷으로 복원됩니다. 복원 적용 직전의 현재 상태도 자동 백업됩니다.

## MCP 연결 (AI 클라이언트)

LifeOps 서버가 실행 중이면 Streamable HTTP 엔드포인트인 `/mcp`에 Claude Code 같은 MCP 클라이언트를 연결할 수 있습니다. MCP는 스키마·엔티티·페이지를 읽고 엔티티를 생성·수정·삭제하는 도구 9종과, 자유 텍스트를 기존 타입의 엔티티로 분해하는 `ingest` 프롬프트를 제공합니다.

### 주소와 포트 확인

서버 시작 로그의 `LifeOps 서버 http://...`에서 실제 주소와 포트를 확인하세요. 기본 포트는 `3000`이지만 이미 사용 중이면 `3001`부터 사용 가능한 포트로 자동 폴백합니다. 서버가 `3000` 포트에서 실행 중일 때는 다음 응답의 `port`로도 확인할 수 있습니다.

```sh
curl -s http://127.0.0.1:3000/api/system/info
```

아래 등록 명령의 `<포트>`를 확인한 실제 포트로 바꾸세요.

### Claude Code 등록

토큰을 설정하지 않은 기본 로컬 실행에서는 loopback 주소에서만 MCP 접속을 허용합니다.

```sh
claude mcp add lifeops --transport http http://127.0.0.1:<포트>/mcp
claude mcp list
```

`LIFEOPS_MCP_TOKEN`을 설정한 서버에는 같은 토큰을 `Authorization` 헤더로 전달해야 합니다.

```sh
claude mcp add lifeops --transport http http://127.0.0.1:<포트>/mcp \
  --header "Authorization: Bearer $LIFEOPS_MCP_TOKEN"
```

### Claude Desktop 지원 범위

[Anthropic의 원격 Custom Connector 안내](https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp)에 따르면 Claude Desktop도 Anthropic 클라우드에서 원격 MCP에 접속하므로 이 로컬 전용 `localhost`/LAN 엔드포인트에 직접 연결할 수 없습니다. 로컬 Desktop 설정은 stdio 또는 Desktop Extension 경로가 필요하지만 LifeOps v14는 Streamable HTTP만 제공합니다. 따라서 v14의 직접 지원 대상은 Claude Code와 로컬 HTTP MCP 클라이언트이며, Desktop용 검증된 stdio 브리지/MCPB는 후속 작업입니다. 개인 LifeOps 데이터를 공개 터널로 노출해 우회하지 마세요.

### 인증

- `LIFEOPS_MCP_TOKEN`이 없으면 `/mcp`는 `127.0.0.1` 또는 `::1`에서 온 요청만 허용하고, 그 밖의 요청은 `403 Forbidden`으로 거부합니다.
- 토큰을 설정하면 접속 위치와 관계없이 `/mcp`의 모든 요청에 `Authorization: Bearer <토큰>`이 필요하며, 없거나 일치하지 않으면 `401 Unauthorized`로 거부합니다.
- LAN에서 MCP를 사용하려면 충분히 길고 무작위인 토큰을 설정한 상태로 서버를 시작하세요.

```sh
export LIFEOPS_MCP_TOKEN="충분히-길고-무작위인-토큰"
cargo run -p lifeops-server
```

같은 셸에서 위 환경변수를 유지한 채 Claude Code 등록 명령도 실행하세요. 서버 시작 후 VPN·네트워크 인터페이스나 IP가 바뀌면 안전한 Host 허용 목록을 갱신하도록 LifeOps를 재시작하고, LAN에서는 호스트명 대신 표시된 IP 주소로 연결하세요.

이 토큰은 `/mcp`만 보호합니다. 웹 UI와 REST API의 LAN 접속에는 별도 인증이 없으므로 기존 안내대로 신뢰하는 네트워크나 개인 VPN에서만 LAN 접속을 사용하세요.

### 쓰기 안전장치

- MCP가 쓸 수 있는 대상은 **엔티티만**입니다. 타입·스키마·페이지의 생성·수정·삭제는 제공하지 않으며 앱 UI에서 사람이 관리합니다.
- 엔티티 쓰기는 기존 웹 UI와 같은 스키마 검증, 참조 삭제 보호, 반복 동작을 거칩니다.
- 검증을 위반한 데이터는 저장하지 않고 필드별 오류를 반환합니다.
