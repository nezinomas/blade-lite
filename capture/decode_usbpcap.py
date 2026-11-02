"""Pull the 90-byte Razer reports out of USBPcap files.

Usage: python capture/decode_usbpcap.py capture/raw/03-preset-performance.hub1.pcap ...
Writes one fixture per input to tests/fixtures/<step>.txt:
  > <90 bytes hex>   host -> device (SET_REPORT), with the interface number
  < <90 bytes hex>   device -> host (GET_REPORT reply)
Only class requests of exactly 90 bytes are kept, so key presses are dropped.
Fan RPM polling is kept only in the idle step, and repeated frame rows once.
"""

import struct
import sys
from pathlib import Path

REPORT_LEN = 90
LINKTYPE_USBPCAP = 249
CONTROL = 2
STAGE_SETUP, STAGE_DATA, STAGE_COMPLETE = 0, 1, 3
SET_REPORT = (0x21, 0x09)
GET_REPORT = (0xA1, 0x01)
FAN_RPM_GET = 0x0D88
FRAME_ROW_SET = 0x030B


def read_pcap(data: bytes):
    magic, = struct.unpack_from('<I', data, 0)
    if magic != 0xA1B2C3D4:
        raise ValueError('not a little-endian pcap file')
    linktype, = struct.unpack_from('<I', data, 20)
    if linktype != LINKTYPE_USBPCAP:
        raise ValueError(f'link type {linktype}, expected USBPcap')
    pos = 24
    while pos + 16 <= len(data):
        _, _, incl, _ = struct.unpack_from('<IIII', data, pos)
        yield data[pos + 16:pos + 16 + incl]
        pos += 16 + incl


def parse_packet(pkt: bytes):
    """Returns (irp, device, stage, from_device, payload) for a control packet, else None."""
    hlen, irp, _status, _func, info, _bus, device, _ep, transfer, _dlen = \
        struct.unpack_from('<HQIHBHHBBI', pkt, 0)
    if transfer != CONTROL or hlen < 28:
        return None
    stage = pkt[27]
    return irp, device, stage, info & 1, pkt[hlen:]


def extract_reports(packets):
    """Yields (direction, interface, report) in capture order."""
    pending, pending_set = {}, {}
    for pkt in packets:
        parsed = parse_packet(pkt)
        if not parsed:
            continue
        irp, device, stage, from_device, payload = parsed
        if stage == STAGE_SETUP and len(payload) >= 8:
            req_type, req, _value, index, length = struct.unpack_from('<BBHHH', payload, 0)
            if length != REPORT_LEN:
                continue
            if (req_type, req) == SET_REPORT and len(payload) >= 8 + REPORT_LEN:
                yield '>', index, payload[8:8 + REPORT_LEN]
            elif (req_type, req) == SET_REPORT:
                pending_set[(device, irp)] = index
            elif (req_type, req) == GET_REPORT:
                pending[(device, irp)] = index
        elif stage == STAGE_DATA and not from_device:
            index = pending_set.pop((device, irp), None)
            if index is not None and len(payload) >= REPORT_LEN:
                yield '>', index, payload[:REPORT_LEN]
        elif stage == STAGE_COMPLETE and from_device:
            index = pending.pop((device, irp), None)
            if index is not None and len(payload) >= REPORT_LEN:
                yield '<', index, payload[:REPORT_LEN]


def command(report: bytes) -> int:
    return report[6] << 8 | report[7]


def trim(reports, keep_polling: bool):
    """Drops what every capture has, so a fixture shows its own step.

    Fan RPM polling (0D88) goes unless keep_polling. Lighting frame rows
    (030B) are kept once each, compared without the transaction ID.
    """
    seen_rows, kept = set(), []
    for direction, mi, rep in reports:
        cmd = command(rep)
        if cmd == FAN_RPM_GET and not keep_polling:
            continue
        if cmd == FRAME_ROW_SET:
            body = rep[2:]
            if body in seen_rows:
                continue
            seen_rows.add(body)
        kept.append((direction, mi, rep))
    return kept


def to_lines(reports):
    return [f'{d} mi={mi} {r.hex(" ")}' for d, mi, r in reports]


def step_name(path: Path) -> str:
    return path.name.split('.')[0]


def main(paths):
    out_dir = Path(__file__).resolve().parent.parent / 'tests' / 'fixtures'
    out_dir.mkdir(parents=True, exist_ok=True)
    for p in map(Path, paths):
        reports = extract_reports(read_pcap(p.read_bytes()))
        lines = to_lines(trim(reports, keep_polling='idle' in p.name))
        if not lines:
            print(f'{p.name}: no 90-byte reports')
            continue
        target = out_dir / f'{step_name(p)}.txt'
        target.write_text('\n'.join(lines) + '\n', encoding='ascii')
        print(f'{p.name}: {len(lines)} reports -> {target.name}')


if __name__ == '__main__':
    main(sys.argv[1:])
