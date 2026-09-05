# 단계 0 — rhwp devel 독해 보고서

읽은 트리: 형제 클론 `C:\Users\swsz9\rhwp` 브랜치 `devel` (blob-filter sparse). GUI·studio·확장·hwpctl은 읽지 않았다. rhwp 함수는 복사하지 않았다.

§1 주장과 다른 실측은 각 절 머리에 적는다.

## (a) 포맷별 레코드·태그와 IR 매핑

### HWP5 (OLE/CFB)

레코드 헤더: 한글 문서 파일 형식 5.0 r1.3 §4 — tag 10bit, level 10bit, size 12bit, size=`0xFFF`이면 추가 u32. 태그 원점 `HWPTAG_BEGIN = 0x10` (`rhwp/src/parser/tags.rs:6`).

| Tag | 값 | 스펙 이름 | DocAgent IR |
| --- | ---: | --- | --- |
| DOCUMENT_PROPERTIES | 16 | 문서 속성 | `Document` 메타 |
| ID_MAPPINGS | 17 | ID 개수 표 | 스타일/폰트 카탈로그 |
| BIN_DATA | 18 | 바이너리 | `ImageData.bytes` |
| FACE_NAME | 19 | 글꼴 | `FontFace` |
| BORDER_FILL | 20 | 테두리/채우기 | `Border` / `TableBorders` |
| CHAR_SHAPE | 21 | 글자 모양 | `CharStyle` |
| TAB_DEF | 22 | 탭 | `RunContent::Inline(Tab)` |
| NUMBERING | 23 | 번호 | `NumberingDef` |
| BULLET | 24 | 글머리표 | `NumberingDef` |
| PARA_SHAPE | 25 | 문단 모양 | `Paragraph` 필드 |
| STYLE | 26 | 스타일 | `NamedStyle` |
| PARA_HEADER | 66 | 문단 헤더 | `Paragraph` |
| PARA_TEXT | 67 | UTF-16LE 본문 | `RunContent::Text` |
| PARA_CHAR_SHAPE | 68 | 글자모양 구간 | `Run.style` |
| PARA_LINE_SEG | 69 | 줄 캐시 | `LayoutHint` (조판 비소비) |
| PARA_RANGE_TAG | 70 | 영역 태그 | 진단 `Unsupported` |
| CTRL_HEADER | 71 | 컨트롤 | 표/구역/머리말 등 |
| LIST_HEADER | 72 | 리스트 | 셀/머리말 문단 묶음 |
| PAGE_DEF | 73 | 용지 | `PageSetup` |
| FOOTNOTE_SHAPE | 74 | 각주/미주 모양 | `Note` |
| PAGE_BORDER_FILL | 75 | 쪽 테두리 | `Background` |
| SHAPE_COMPONENT | 76 | 그리기 | `Float::Shape` |
| TABLE | 77 | 표 | `Table` |
| SHAPE_* (line…picture) | 78–85 | 도형 | `ShapeKind` |
| CTRL_DATA | 87 | 컨트롤 데이터 | 확장 `Option` |
| EQEDIT | 88 | 수식 | `Float::Equation` |
| MEMO_SHAPE / LIST | 92–93 | 메모 | `Diagnostic` |
| CHART_DATA | 95 | 차트 | 범위 밖(스프레드시트 아님, 미지원) |

컨트롤 fourcc (`tags.rs:132+`): `secd` 구역, `cold` 단, `tbl ` 표, `eqed` 수식, `gso ` 그리기, `head`/`foot`, `fn  `/`en  `, `atno`/`nwno`, `pgnp`/`pgct`/`pghd`, 필드 `%clk` `%hlk` 등.

인라인 코드: `0x0002` 구역/단, `0x000B` 확장 컨트롤(8 WCHAR), `0x000D` 문단 끝 (`tags.rs:108–125`).

### HWPX (OWPML ZIP+XML)

파서 위치 `src/parser/hwpx/`. 핵심 태그:

| 원소 | IR |
| --- | --- |
| `hs:sec` | `Section` |
| `hp:p` / `hp:run` / `hp:t` | `Paragraph` / `Run` / text |
| `hp:tbl` / `hp:tr` / `hp:tc` | `Table` |
| `hp:pagePr` / `hp:margin` | `PageSetup` |
| `hp:lineseg` / `hp:linesegarray` | `LayoutHint` |
| `hh:head` | 스타일 카탈로그 |
| `hp:header` / `hp:footer` | `HeaderFooter` |

패키지 엔트리: `Contents/sectionN.xml`, `Contents/header.xml`, `Contents/content.hpf`, `version.xml`.

### HML

`src/parser/hml/`. 원소 `HWPML` / `BODY` / `SECTION` / `P` / `TEXT` / `TABLE` / `TR` / `TD` → 동일 IR. 페이지 속성은 `PageWidth` 등 속성.

### HWP3

`src/parser/hwp3/` (`mod.rs`, `records.rs`, `paragraph.rs`, `johab.rs`…). 시그니처 `HWP Document File V3.00` + `1A 01 02 03 04 05`. 조판 분기는 파서에서 IR 속성으로 끝내야 한다 (`parser_architecture.md` HWP3 불변식).

## (b) `mydocs/tech/parser_architecture.md` 요약

출처: `rhwp/mydocs/tech/parser_architecture.md` (canonical, last_verified 2026-08-12).

- HWPX·HWP5·HWP3 파서는 공통 `Document` IR로만 내보낸다. 렌더러는 입력 포맷을 다시 보지 않는다.
- 압축 폭탄 예산: 단일 스트림 256 MiB, HWP5 DocInfo+본문 누적 512 MiB. 초과는 부분 문서가 아니라 오류.
- 예산 선택은 문서 열기 진입점에만 있다. CFB decode API는 상한을 import하지 않는다.
- 비공개 10k 코퍼스에서 이 예산 초과 0건. 실측 최대 약 20.67 MiB (호환 근거이지 규격 상한 증명 아님).
- HWP3 전용 분기는 `src/parser/hwp3/` 안에서 끝. `src/renderer/`, `layout.rs`, `document_core/`에 HWP3 분기 금지.
- 소스 계보는 `Document.provenance` / `layout_profile()` 질의. boolean 필드 직접 읽기 금지 (#2403 정책 — 이슈 번호는 rhwp 문서의 인용).

**§1과의 차이:** CLAUDE.md가 “렌더러에 HWP3 분기 금지”인데 아키텍처 문서는 같은 금지를 명시한다. `typeset.rs`의 hwp3 언급은 이 독해 범위의 `typeset.rs` import 목록에서 직접 세지 않았고, 금지와 구현의 괴리는 §1 주장으로 남겨 단계 0에서 렌더러 전수를 하지 않았다(29k줄). 듀얼 엔진 주석은 확인했다 (`typeset.rs:3–5`).

## (c) LineSeg 오라클

`rhwp/src/renderer/composer/lineseg_compare.rs:1–56`.

원본 `LINE_SEG`와 `reflow_line_segs()`를 필드별로 뺀다. 비교 필드: `text_start`, `line_height`, `text_height`, `baseline_distance`, `line_spacing`, `segment_width`, `vertical_pos` (`:10–18`).

`all_match`는 `vertical_pos`를 제외한다 (`:28–35`) — y 위치는 줄바꿈 일치와 별개.

역할: **오라클·점수**. 조판이 LineSeg를 입력으로 소비한다는 계약은 이 파일에 없다. DocAgent는 IR `LayoutHint`로 보존하고 `docagent-layout`은 읽지 않는다.

레코드 크기: 본 구현은 한글 5.0 본문 레이아웃 캐시 관례 36바이트(`textpos, y, line_height, text_height, baseline, spacing, x, width, flags`).

## (d) 튜닝 상수

§1은 “76개 (`_TOLERANCE_PX`, `_GUARD_PX`, `_BLEED_PX`)”를 주장. **실측:** `src/renderer`에서 `const …_PX` 정의 **79건**(중첩 함수 중복 이름 포함). 스펙이 꼽은 세 접미사보다 넓다. `typeset.rs`만 29건.

| 파일:줄 | 이름 | 값 | 주석 근거 |
| --- | --- | ---: | --- |
| typeset.rs:56 | ENDNOTE_PAGE_OFFCANVAS_GUARD_PX | 56.0 | §1이 인용한 샘플 곡선. 소스 한 줄 주석은 값만. |
| typeset.rs:767 | MIN_TOP_KEEP_PX | 25.0 | |
| typeset.rs:2933 | FLOW_SOURCE_TOLERANCE_PX | 2.0 | |
| typeset.rs:2934 | FOOTNOTE_BOUNDARY_TOLERANCE_PX | 0.5 | |
| typeset.rs:3598 | SOURCE_FRAME_EPSILON_PX | 0.5 | |
| typeset.rs:3617 | SAVED_FRAME_ROW_END_STORED_TOLERANCE_PX | 1.0 | |
| typeset.rs:3889 | LADDER_FIT_EPSILON_PX | 1.0 | |
| typeset.rs:4172 | SAVED_LINE_FLOW_ANCHOR_TOLERANCE_PX | 16.0 | |
| typeset.rs:4176 | SAVED_FRAME_FLOW_DRIFT_TOLERANCE_PX | 64.0 | |
| typeset.rs:4180 | BODY_BOTTOM_SEAT_PX | 10.0 | |
| typeset.rs:4185–4191 | TERMINAL_ROW_BOTTOM_SQUEEZE_* | 13 / 100 / 12 | |
| typeset.rs:9109 | PUSHDOWN_GAP_TOL_PX | 8.0 | layout.rs:12306 과 중복 |
| typeset.rs:17165–6 | *_LAYOUT_DRIFT_SAFETY_PX | 4.0 / 0.0 | engine.rs:440 과 계열 |
| typeset.rs:22634 | MIXED_NESTED_OWNER_DRIFT_MIN_PX | 16.0 | |
| typeset.rs:23562 | DECLARED_FLOAT_FIT_TOLERANCE_PX | 1.0 | |
| typeset.rs:23577 | ANCHOR_DELAY_FLOAT_EPS_PX | 1e-6 | |
| typeset.rs:23644 | NATIVE_HWP5_NEAR_ANCHOR_ROWBREAK_FRAGMENT_TOLERANCE_PX | 24.0 | 포맷 분기 상수 |
| typeset.rs:24073 | NEAR_MEASURED_ROWBREAK_FIT_PX | 2.0 | |
| typeset.rs:24502 | NATIVE_REWIND_FIRST_FRAGMENT_PAINT_FOOTER_GUARD_PX | 4.0 | |
| typeset.rs:25728 | WHOLE_TABLE_FIT_TOLERANCE_PX | 2.0 | |
| typeset.rs:25779–81 | LANDSCAPE_ROWBREAK_* | 36 / **260** / 260 | §1 인용과 일치 |
| typeset.rs:25782–84 | HWPX_LANDSCAPE_ROWBREAK_* | 48 / **320** / 320 | §1 “HWPX 버전은 320”과 일치 |
| layout.rs:2029 | ENDNOTE_COLUMN_BOTTOM_BLEED_TOLERANCE_PX | 24.0 | typeset가 import (`typeset.rs:35`) |
| layout.rs:2032 | ENDNOTE_LAST_COLUMN_SPLIT_BLEED_PX | 4.0 | |
| layout.rs:2044–46 | ENDNOTE_*_OVERFLOW_LOG_TOLERANCE_PX | 48 / 68 / 33 | |
| layout.rs:1357 | MAX_REWIND_DRIFT_PX | 24.0 | |
| layout.rs:2229 | TABLE_OVERLAP_THRESHOLD_PX | 2.0 | |
| layout.rs:2408–10 | MAX_BACKWARD_PX / MAX_TABLE_HOST_FORWARD_PX | 8 / 100 | |
| layout.rs:9226 | TAC_LEADING_OVERHANG_TOLERANCE_PX | 16.0 | |
| table_layout.rs:20 | ROWBREAK_OBJECT_BOTTOM_BLEED_TOLERANCE_PX | 64.0 | |
| table_layout.rs:33 | ROW_CUT_CAPACITY_FP_EPSILON_PX | 0.1 | |
| table_layout.rs:36 | PAGE_SCALE_CELL_HEIGHT_PX | 800.0 | |
| table_layout.rs:474–496 | NESTED_*_EPSILON_PX 계열 | 0.5 / 0.05 / 6 | |
| table_layout.rs:11474+ | SLIVER_ABSORB_OVERFLOW_TOLERANCE_PX | 48.0 | 함수 지역 중복 |
| table_layout.rs:12364+ | HARD_BREAK_REMAINING_TOLERANCE_PX | 32.0 | 지역 중복 |
| float_placement.rs:1077 | MIN_FRAGMENT_KEEP_PX | 25.0 | |
| height_cursor.rs:1412 | SYNTH_FORWARD_REANCHOR_MIN_PX | 48.0 | |
| height_measurer.rs:2918 | TAC_FLOOR_OVERFLOW_NOSHRINK_CAP_PX | 48.0 | |
| engine.rs:440 | LAYOUT_DRIFT_SAFETY_PX | 4.0 | |
| engine.rs:2752 | MIN_SPLIT_CONTENT_PX | 10.0 | |
| shape_layout.rs:445 | MATRIX_TEXT_FIT_TOLERANCE_PX | 3.0 | |
| border_rendering.rs:729 | PAINT_INSET_EPSILON_PX | 0.05 | |
| render_tree.rs:57 | TAB_DOT_LEADER_DASH_PX | 0.1 | |
| svg.rs:3994 | HU_PER_PX | 75.0 | 단위 변환. 96dpi면 75 HU/px (7200/96). |

분류: **한컴 규칙에 가까운 것** — `HU_PER_PX=75` (96dpi). **커브피팅** — landscape short-row 260 vs HWPX 320, endnote off-canvas 56, squeeze 13/100/12. DocAgent는 이 값을 이식하지 않는다.

## (e) §5 기여물 목록

`{{내 핸들}}`은 프롬프트에 **미기입**. author 필터 `git log --author`는 실행하지 않았다(핸들을 지어내지 않음). 클론이 `--depth 1`이라 전체 역사도 없다.

HEAD에서 해당 경로를 만진 커밋 저자(단정하지 않음): `Taesup Jang <tsjang@gmail.com>`.

| 지정 경로 | 존재 | 비고 |
| --- | --- | --- |
| `src/agent/` | 예 | `mod.rs`, `dsel/` (ast, eval, lex, parse, glob, suggest, token, tests) |
| `src/capsule_sign.rs` | 예 | 캡슐 서명 → `docagent-capsule` |
| `src/lineage_bundle.rs` | 예 | 계보 |
| `src/audit_standard.rs` | 예 | 감사 |
| `src/anchor_log.rs` | 예 | |
| `src/policy_gate.rs` | 예 | v1 인터페이스만 |
| `src/plan_schema.rs` | 예 | 계획 해시 입력 |
| `src/capabilities_schema.rs` | 예 | |
| `src/agent_profiles.rs` | 예 | |
| `src/agent_seal.rs` | 예 | |
| `src/disclose.rs` | 예 | v1 밖, trait만 |
| `src/settle.rs` | 예 | v1 밖, trait만 |
| `src/mcp_serve.rs` | 예 | → `docagent-mcp` |
| `mydocs/tech/standards/agent_work_standard.{md,json}` | 예 | |
| `tests/agent_*_contract.rs` | 예 | `agent_codex_contract.rs`, `agent_context_cost_contract.rs`, `agent_profile_router_contract.rs`, `agent_toolkit_contract.rs` |
| `tools/agent-toolkit/` | 예 | |
| `tools/agent_bench/` | 예 | |
| `{{추가 경로}}` | 미기입 | 추가하지 않음 |

이식 원칙: 3해시·계보·감사는 `docagent-api` 기본. 정산·선택 공개·PQ는 인터페이스만 (`Settlement`, `OptionalDisclose`).

## (f) HWP ↔ OOXML 문단·런·표 초안

| 개념 | HWP5 | HWPX | OOXML (ECMA-376) | IR |
| --- | --- | --- | --- | --- |
| 문서 | CFB | ZIP package | OPC ZIP | `Document` |
| 구역 | `secd` + `PAGE_DEF` | `hs:sec` + `hp:pagePr` | `w:sectPr` / `w:pgSz` / `w:pgMar` | `Section` + `PageSetup` |
| 문단 | `PARA_HEADER`+`PARA_TEXT` | `hp:p` | `w:p` | `Paragraph` |
| 런 | `PARA_CHAR_SHAPE` 구간 | `hp:run`/`hp:t` | `w:r`/`w:t` | `Run` |
| 정렬 | PARA_SHAPE | `hp:p` 속성 | `w:jc` | `Alignment` |
| 줄간격 | PARA_SHAPE | paraPr | `w:spacing`/`w:lineRule` | `LineSpacing` |
| 표 | `tbl ` + `TABLE` + 셀 LIST | `hp:tbl/tr/tc` | `w:tbl/tr/tc` | `Table` |
| 셀 병합 | 셀 속성 | span 속성 | `w:gridSpan` / `w:vMerge` | `CellMerge` |
| 머리/꼬리말 | `head`/`foot` | header/footer | `w:hdr`/`w:ftr` | `HeaderFooter` Option들 |
| 각주/미주 | `fn  `/`en  ` | footnote/endnote | `w:footnote`/`w:endnote` | `Note` |
| 줄 캐시 | `PARA_LINE_SEG` | `hp:lineseg` | 없음 | `LayoutHint` (DOCX에서 손실 진단) |
| 단위 | HWPUNIT 1/7200 in | 동일 | twip 1/1440 in | HWPUNIT, twip×5 |

DOCX에만 있는 `w:outlineLvl`, `w:keepNext` 등은 IR에 명시적 `Option`. HWP에만 있는 LineSeg는 DOCX 수출 시 `DiagnosticCode::RoundtripLoss`.

## §1 실측 대조

| 주장 | 이 클론 |
| --- | --- |
| `typeset.rs` 29,405줄 | **28,782줄** (`devel` sparse HEAD) |
| 레이아웃 엔진 둘 | **확인.** `typeset.rs:3–5`가 `height_measurer → pagination → layout`을 구형으로 지목하고 TypesetEngine을 신형으로 둔다. `layout.rs`가 살아 있다. |
| 튜닝 76개 | renderer `const *_PX` **79** (중복 지역 const 포함) |
| LineSeg 오라클 | `lineseg_compare.rs` 존재, 개념만 참조 |
| 포맷 식별자 렌더러 침투 | landscape 상수가 HWP5/HWPX로 갈림 (`HWPX_LANDSCAPE_*`) — DocAgent 금지 목록의 근거 |

단어 수는 이 절까지 5,000 미만이다.
