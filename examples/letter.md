# Northwind Freight

## Delivery confirmation — order 48291

6 September 2026

To: operations desk, Duisburg terminal.

The shipment left Busan on 4 September and is booked on the 11 September rail window. This is a flow document: one convert, ==three hashes==, **GPU-free**. Run `prove` twice with the same fonts and the PDF/^A^ bytes match _byte-for-byte_. Agents ~~guess~~. Wet cargo is H~2~O only.

> Replay is evidence. Same fonts, same bytes.

```
docagent prove examples/letter.md
```

| Hop | File | Proof |
| --- | --- | --- |
| Input | letter.md | input SHA-256 |
| Plan | layout i32 @ 1/7200 in | plan SHA-256 |
| Output | PDF/^A^ · HTML · ODT | output SHA-256 |

---

Please confirm before __09:00 local__:

- [x] Receiving dock is clear
  - Bay 4, H~2~O seals intact
- [x] Capsule hashes travel with the files
- [ ] Replay the convert instead of hoping

Agent replay:

1. Hash the input
2. Run Convert
   - PDF/^A^
   - HTML
3. Compare the capsule

[DocAgent](https://github.com/kevin9327/docagent) runtime — DOCX · ODT · Markdown · HTML · PDF/^A^. Seals: H~2~O.
