# DocAgent

에이전트가 흐름 문서를 읽고·기안하고·변환하고·검증하는 결정론 런타임이다.
사람은 PDF / HWP / HWPX / DOCX 결과물만 받는다.

진입점은 `Command` / `Query` / `Event` 하나다. CLI, MCP, WIT, REST(`docagentd`)는 얇은 어댑터다.
모든 Command는 입력·계획·출력 3해시 캡슐을 남긴다.

```
docagent convert in.hwp --pdf out.pdf --html out.html --ir out.json --capsule cap.json
```
