import argparse
import csv
import re
import sys
from pathlib import Path

def read_text(path):
    data = path.read_bytes()
    if data.startswith(b"\xff\xfe"):
        return data[2:].decode("utf-16-le", errors="replace")
    if data.startswith(b"\xfe\xff"):
        return data[2:].decode("utf-16-be", errors="replace")
    if data.startswith(b"\xef\xbb\xbf"):
        return data[3:].decode("utf-8", errors="replace")
    return data.decode("utf-8", errors="replace")

SEQ_RE = re.compile(
    r"\[(?P<idx>\d+)\]\s+seq=(?P<seq>\d+)\s+ts=(?P<ts>\d+)\s+len=(?P<len>\d+)"
)

TM_RE = re.compile(
    r"\[(?P<idx>\d+)\]\s+src=0x(?P<src>[0-9a-fA-F]+)\s+when=(?P<when>\d+)ms\s+"
    r"tm=Telemetry\s*\{\s*"
    r"node_addr:\s*(?P<node_addr>\d+)\s*,\s*"
    r"sample_id:\s*(?P<sample_id>\d+)\s*,\s*"
    r"beacon_timestamp:\s*(?P<beacon_timestamp>\d+)\s*,\s*"
    r"temp:\s*(?P<temp>[^\s,}]+)\s*,\s*"
    r"bat_voltage:\s*(?P<bat_voltage>[^\s,}]+)\s*"
    r"\s*\}"
)

RAW_RE = re.compile(
    r"\[(?P<idx>\d+)\]\s+src=0x(?P<src>[0-9a-fA-F]+)\s+when=(?P<when>\d+)ms\s+"
    r"payload\(raw\)=(?P<raw>.*)"
)

COLUMNS = [
    "idx", "seq", "ts", "src", "when_ms",
    "node_addr", "sample_id", "beacon_timestamp", "temp", "bat_voltage",
    "status", "raw",
]


def to_float(text):
    try:
        return float(text)
    except ValueError:
        return text


def parse(path):
    meta = {}
    decoded = {}
    raw = {}

    for line in read_text(path).splitlines():
        m = SEQ_RE.search(line)
        if m:
            idx = int(m.group("idx"))
            meta[idx] = {
                "seq": int(m.group("seq")),
                "ts": int(m.group("ts")),
                "len": int(m.group("len")),
            }
            continue

        m = TM_RE.search(line)
        if m:
            idx = int(m.group("idx"))
            decoded[idx] = {
                "src": "0x" + m.group("src"),
                "when_ms": int(m.group("when")),
                "node_addr": hex(int(m.group("node_addr"))),
                "sample_id": int(m.group("sample_id")),
                "beacon_timestamp": int(m.group("beacon_timestamp")),
                "temp": to_float(m.group("temp")),
                "bat_voltage": to_float(m.group("bat_voltage")),
                "status": "ok",
            }
            continue

        m = RAW_RE.search(line)
        if m:
            idx = int(m.group("idx"))
            raw[idx] = {
                "src": int(m.group("src"), 16),
                "when": int(m.group("when")),
                "status": "decode-failed",
                "raw": m.group("raw"),
            }

    idxs = sorted(set(meta) | set(decoded) | set(raw))
    rows = []
    for idx in idxs:
        row = {c: "" for c in COLUMNS}
        row["idx"] = idx
        if idx in meta:
            row["seq"] = meta[idx]["seq"]
            row["ts"] = meta[idx]["ts"]
        if idx in decoded:
            row.update(decoded[idx])
        elif idx in raw:
            row.update(raw[idx])
        rows.append(row)
    return rows


def write_xlsx(rows, out):
    from openpyxl import Workbook

    wb = Workbook()
    ws = wb.active
    ws.title = "telemetry"
    ws.append(COLUMNS)
    for row in rows:
        ws.append([row[c] for c in COLUMNS])
    wb.save(out)


def write_csv(rows, out):
    with open(out, "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(COLUMNS)
        for row in rows:
            w.writerow([row[c] for c in COLUMNS])


def main(argv):
    ap = argparse.ArgumentParser(description="flash_read log -> Excel/CSV")
    ap.add_argument("input", help="flash_read console log text file")
    ap.add_argument("-o", "--output", help="output file")
    ap.add_argument("--csv", action="store_true", help="write CSV instead of xlsx")
    args = ap.parse_args(argv)

    src = Path(args.input)
    if not src.exists():
        sys.exit(f"no such file: {src}")

    rows = parse(src)

    if not args.output:
        ext = "csv" if args.csv else "xlsx"
        args.output = src.with_suffix("." + ext)

    if args.csv:
        write_csv(rows, args.output)
    else:
        write_xlsx(rows, args.output)

    print(f"wrote {len(rows)} rows to {args.output}")


if __name__ == "__main__":
    main(sys.argv[1:])
