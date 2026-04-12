# SparkId Format Specification

## String Format

A SparkId is a **21-character fixed-length Base58 string** composed of three segments:

```
[8 timestamp][6 counter][7 random]
 ─────┬─────  ────┬────  ────┬───
   c0–c7       c8–c13     c14–c20
```

### Alphabet

```
123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz
```

58 characters — digits `1`–`9`, uppercase `A`–`Z` excluding `I` and `O`, lowercase `a`–`z` excluding `l`. Each character maps to an **alphabet index** 0–57 by its position in the string above.

### Segments

| Segment   | Characters | Description                                                                 |
| --------- | ---------- | --------------------------------------------------------------------------- |
| Timestamp | c0–c7      | Milliseconds since Unix epoch, Base58-encoded big-endian                    |
| Counter   | c8–c13     | Randomly seeded each new millisecond, incremented per-ID within the same ms |
| Random    | c14–c20    | Cryptographically random Base58 characters                                  |

### Sort Order

The alphabet is a subsequence of ASCII in ascending order — `'1'`(0x31) < `'2'`(0x32) < … < `'z'`(0x7A). Lexicographic string comparison yields chronological ordering for IDs from a single generator.

---

## Binary Format

A **128-bit (16-byte)** packed encoding of the 21-character string.

Each character's alphabet index (0–57) fits in 6 bits ($2^6 = 64 > 58$). Pack all 21 indices consecutively, MSB-first:

$$21 \times 6 = 126 \text{ bits} + 2 \text{ bits padding} = 128 \text{ bits} = 16 \text{ bytes}$$

### Bit Layout

```
128 bits, big-endian (MSB at byte 0)

 ┌─────┬─────┬─────┬─────┬─────┬─────┬─────┬─────┬─────┬─────┬─────┐
 │ c0  │ c1  │ c2  │ c3  │ c4  │ c5  │ c6  │ c7  │ c8  │ c9  │ c10 │
 │6 bit│6 bit│6 bit│6 bit│6 bit│6 bit│6 bit│6 bit│6 bit│6 bit│6 bit│
 ├─────┼─────┼─────┼─────┼─────┼─────┼─────┼─────┼─────┼─────┼─────┤
 │ c11 │ c12 │ c13 │ c14 │ c15 │ c16 │ c17 │ c18 │ c19 │ c20 │ pad │
 │6 bit│6 bit│6 bit│6 bit│6 bit│6 bit│6 bit│6 bit│6 bit│6 bit│2 bit│
 └─────┴─────┴─────┴─────┴─────┴─────┴─────┴─────┴─────┴─────┴─────┘
```

### Byte-Level Packing

Every 3 bytes holds exactly 4 character indices ($4 \times 6 = 24 = 3 \times 8$). The 21 characters divide into 5 groups of 4, plus 1 trailing character:

```
Bytes  0–2:   c0  c1  c2  c3     (group 0)
Bytes  3–5:   c4  c5  c6  c7     (group 1)
Bytes  6–8:   c8  c9  c10 c11    (group 2)
Bytes  9–11:  c12 c13 c14 c15    (group 3)
Bytes 12–14:  c16 c17 c18 c19    (group 4)
Byte  15:     c20 + 2-bit pad    (tail)
```

Within each group, the four indices are packed MSB-first: the first index occupies bits 23–18, the second 17–12, the third 11–6, the fourth 5–0. The tail byte places its index in bits 7–2, with bits 1–0 set to zero.

### Sort Order Preservation

Because the alphabet indices (0–57) are ordered identically to the corresponding ASCII bytes, and indices are packed MSB-first, byte-wise comparison of the 16-byte binary gives the **exact same ordering** as lexicographic comparison of the 21-character string.
