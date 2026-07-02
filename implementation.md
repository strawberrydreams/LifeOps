# Implementation Notes

## 2026-07-02

- Task 3 시작: `view::query`는 요구된 TDD 순서대로 먼저 필터/정렬 테스트를 추가하고, `view::mod`에서 `query`와 `run_view`를 노출했다.
- red 준비: production 구현 전 테스트 대상이 컴파일되도록 `run_view`는 임시로 `UnknownSource`만 반환하는 최소 stub으로 두었다. 이후 red 실패가 확인되면 실제 필터/정렬 로직으로 교체한다.
- red 확인: `cargo test -p lifeops-core view::query`는 5개 중 unknown source 1개만 통과하고, 나머지는 `UnknownSource("물건")` 또는 없는 필드 메시지 불일치로 실패했다. 테스트가 새 동작을 실제로 요구함을 확인했다.
- 변경: `run_view`는 source/filter/sort 필드를 먼저 검증한 뒤 `SchemaSet::family_of(source)`로 엔티티를 로드하고 Rust에서 필터와 정렬을 적용한다. Task 4 전까지 `aggregates`는 항상 빈 `IndexMap`으로 둔다.
- 결정: operator map은 요구대로 정확히 한 키만 인정한다. `month`, `lt`, `gt`가 아니거나 여러 연산자가 들어오면 에러가 아니라 미매칭 처리한다.
- trade-off: 정렬에서 누락 값은 ascending/descending 모두 마지막에 둔다. descending은 존재하는 값 사이의 순서만 뒤집어 누락 데이터가 화면 앞쪽으로 튀어나오지 않게 했다.
- trade-off: `lt`/`gt`의 date 비교는 ISO 형식 문자열이라는 기존 검증 전제 위에서 lexicographic 비교만 한다. money/number는 공통 `extract_f64`로 숫자와 `{ amount }` 형태를 모두 처리한다.
- green 확인: `cargo test -p lifeops-core view::query`는 5 passed. 이후 `cargo test -p lifeops-core && cargo clippy -p lifeops-core -- -D warnings`도 통과했다.
- 리뷰 수정: `가격: null`처럼 필드는 있지만 sort key가 없는 값이 `sort: -가격`에서 앞쪽으로 뒤집히는 회귀 테스트를 추가했다. 정렬은 먼저 `SortKey::Missing` 여부를 판정해 null/누락 값을 양방향 모두 마지막에 두고, non-missing 값끼리만 descending reverse를 적용한다.
- Task 2 시작: `view::model` 데이터 모델은 먼저 요구된 YAML 파싱 테스트 3개를 추가하고, 테스트가 컴파일 대상에 포함되도록 최소 `view` 모듈만 연결한 뒤 red 상태를 확인하기로 했다.
- red 확인: `cargo test -p lifeops-core view::model`은 `ViewBlock`, `Layout`, `PageDef` 미정의 컴파일 오류로 실패했다. 이는 새 테스트가 실제로 컴파일 대상에 들어왔음을 확인하는 실패다.
- 변경: `view::model`에 `Filter`, `Layout`, `ViewBlock`, `PageDef`, `ViewResult`, `PageResult`를 추가하고 `view::mod`에서 재노출했다. `Layout`은 YAML/API 문자열을 소문자로 유지하고 기본값을 `Table`로 둔다.
- 변경: `error.rs`에 이후 view 로딩/검증 단계에서 쓸 `ViewError`를 추가했다. 이번 Task 2에서는 타입 정의만 필요해 아직 호출 지점은 없다.
- trade-off: 요구된 `ViewError::UnknownField { source: String, ... }` 필드명을 유지하기 위해 `ViewError`만 `thiserror::Error` derive 대신 수동 `Display`/`Error`/`From` 구현을 사용했다. `thiserror` 2.x는 이름이 `source`인 필드를 자동으로 `Error::source()`로 취급해 `String` 필드와 함께 컴파일되지 않는다.
- trade-off: `ViewBlock`의 `filter`, `sort`, `columns`, `aggregate`는 누락 YAML을 허용하기 위해 모두 `Option`과 `#[serde(default)]`를 사용했다. 빈 맵/빈 벡터로 정규화하지 않고 원본 정의의 생략 여부를 보존한다.
- `docs/superpowers/plans/2026-07-02-lifeops-view-api.md`의 Task 6 `AppState`를 최종 형태로 통일했다.
- 결정: `AppState`는 처음 생성될 때부터 `schemas_dir`와 `views_dir`를 보관한다. `/api/reload`가 Task 8에서 추가되지만, 이 경로는 서버 상태의 기본 불변 컨텍스트이므로 Task 8에서 타입 시그니처를 다시 바꾸지 않는다.
- 변경: Task 6의 `test_state()`는 처음부터 `(AppState, tempfile::TempDir)`를 반환하고, Task 6/7 테스트는 `let (state, _dir) = test_state().await;` 형태로 tempdir 수명을 유지한다.
- trade-off: Task 6 시점에는 `schemas_dir`/`views_dir`가 health API에 직접 필요하지 않지만, 초기 타입 정의에 포함해 Task 8에서 앞선 테스트와 호출부를 재수정하는 계획 내 재작업을 제거했다.
- Task 1 이연 항목 처리: `collect_refs`가 `(field, to_id, target_type)`를 반환하도록 바꾸고, `check_ref_targets`가 참조 대상 존재뿐 아니라 실제 엔티티 타입이 선언 target의 `SchemaSet::family_of()`에 포함되는지도 검증한다.
- 결정: ref 필드의 `target`이 resolve 단계에서 보장된다는 전제를 유지하되, 예외적으로 비어 있으면 해당 edge를 만들지 않도록 방어적으로 skip한다. 공개 API 시그니처는 바꾸지 않았다.
- 변경: 타입 불일치 검증 실패 메시지는 한국어로 `타입`을 포함하고, 참조 id, 실제 타입, 기대 target 타입을 함께 담는다.
- 변경: `delete()`의 존재 확인과 backlink 조회를 삭제 트랜잭션 내부로 이동해 검사와 삭제 사이의 TOCTOU 창을 줄였다. 공개 `backlinks()` 메서드는 기존 조회 API로 그대로 둔다.
- trade-off: `delete()` 내부에 backlink 조회 SQL을 한 번 더 두어 중복이 생겼지만, 공개 `backlinks()`가 풀 기반 조회를 유지해야 하므로 트랜잭션 내부 검사용 헬퍼를 새로 공개하지 않는 쪽을 택했다.
- 리뷰 보강: ref 대상 타입 불일치 회귀 테스트가 `create()`와 `update()` 경로를 모두 덮고, 메시지에 `타입`, 참조 id, 실제 타입(`할일`), 기대 target(`물건`)이 포함되는 계약을 검증한다.
