# 포맷 간 파일 이동 검증

요구: DocHWP가 생성·소비하는 포맷 사이에서 **100건 이상**의 파일 이동이 손실 없이 동작할 것.

구현: `crates/dochwp-api/tests/format_hops.rs`

| 항목 | 값 |
| --- | --- |
| 문서 수 | 120 |
| 문서당 홉 | 5 (`IR→HWP5→HWPX→HML→DOCX→HWP5`) |
| 총 이동 | 600 |
| 검사 | 원본 `plain_text()`의 모든 비공백 줄이 도착 IR에 존재 |
| 구동 대상 | shipped `dochwp-hwp5/hwpx/hml/docx::{read,write}` |

실행:

```
cargo test -p dochwp-api --test format_hops
```

표가 있는 문서는 `i % 3 == 0`마다 넣어 표 경로도 같은 홉을 탄다.
