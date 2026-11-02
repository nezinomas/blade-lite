import struct
import unittest

from decode_usbpcap import extract_reports, read_pcap, to_lines

REPORT = bytes([0x00, 0x1F, 0, 0, 0, 0x04, 0x0D, 0x02, 1, 1, 2, 0]) + bytes(78)


def packet(irp, stage, from_device, payload, transfer=2):
    header = struct.pack('<HQIHBHHBBIB', 28, irp, 0, 0, from_device, 1, 5, 0, transfer, len(payload), stage)
    return header + payload


def setup(req_type, req, length=90, index=2):
    return struct.pack('<BBHHH', req_type, req, 0x0300, index, length)


def pcap(packets):
    out = struct.pack('<IHHiIII', 0xA1B2C3D4, 2, 4, 0, 0, 65535, 249)
    for p in packets:
        out += struct.pack('<IIII', 0, 0, len(p), len(p)) + p
    return out


class DecodeTest(unittest.TestCase):
    def test_set_report_with_data_in_setup_packet(self):
        pkts = [packet(1, 0, 0, setup(0x21, 0x09) + REPORT)]
        self.assertEqual(list(extract_reports(pkts)), [('>', 2, REPORT)])

    def test_set_report_with_separate_data_stage(self):
        pkts = [packet(1, 0, 0, setup(0x21, 0x09)), packet(1, 1, 0, REPORT)]
        self.assertEqual(list(extract_reports(pkts)), [('>', 2, REPORT)])

    def test_get_report_reply_matched_by_irp(self):
        pkts = [packet(7, 0, 0, setup(0xA1, 0x01)), packet(7, 3, 1, REPORT)]
        self.assertEqual(list(extract_reports(pkts)), [('<', 2, REPORT)])

    def test_other_lengths_and_transfers_are_dropped(self):
        pkts = [packet(1, 0, 0, setup(0x21, 0x09, length=8) + bytes(8)),
                packet(2, 0, 0, bytes(8), transfer=1)]
        self.assertEqual(list(extract_reports(pkts)), [])

    def test_read_pcap_round_trip_and_line_format(self):
        pkts = list(read_pcap(pcap([packet(1, 0, 0, setup(0x21, 0x09) + REPORT)])))
        line = to_lines(extract_reports(pkts))[0]
        self.assertTrue(line.startswith('> mi=2 00 1f 00 00 00 04 0d 02'))


if __name__ == '__main__':
    unittest.main()
