# 커뮤니티 코덱 가이드 (ODT / RTF / Markdown)

1. `crates/dochwp-codec-template`를 복사해 `dochwp-odt` / `dochwp-rtf` / `dochwp-md`로 이름을 바꾼다.
2. 의존성은 `dochwp-model`과 포맷 파서 크레이트만. `dochwp-layout` 이하는 금지.
3. `sniff` / `read` / `write` 세 함수가 계약이다. 왕복 텍스트와 표를 `proptest`로 잠근다.
4. IR에 포맷 식별자를 넣지 않는다. 포맷 특유 값은 명시적 `Option` 필드로 정규화한다.
5. `dochwp-conformance` 점수판의 파싱 손실 0을 통과한 뒤에만 워크스페이스 멤버로 승격한다.

ODT는 ZIP+`content.xml`, RTF는 `{\rtf`, Markdown은 CommonMark 부분집합부터.
세 코덱의 본문은 v1 제품 범위 밖이다. 이 가이드와 템플릿만 제공한다.
