#!/usr/bin/env python3

# This file is derived from the Cilium project.
# Copyright Authors of Cilium
# SPDX-License-Identifier: Apache-2.0

import importlib
import pkgutil
import sys

import pkt_defs

from scapy.all import Packet, chexdump

MAX_PACKET_SIZE = 1518
PACKET_BYTES_DEFINE_FMT = "#[allow(dead_code)]\npub const SCAPY_{}_BYTES: [u8; {}] = [{}];\n\n"
HEADER_BANNER = '''
//
// This is an auto-generated file containing byte arrays of the scapy
// buffer definitions.
//

'''

class ScapyHeaderGenerator:
    def __init__(self, output_file):
        self.output_file = output_file
        self.defines = []

    def generate_headers(self):
        for module_info in pkgutil.iter_modules(pkt_defs.__path__):
            if not module_info.name.endswith("_pkt_defs"):
                continue

            module = importlib.import_module(
                f"pkt_defs.{module_info.name}"
            )

            for name, pkt in vars(module).items():
                if not isinstance(pkt, Packet):
                    continue

                pkt_bytes = bytes(pkt)
                if len(pkt_bytes) > MAX_PACKET_SIZE:
                    print(f"[Error] Packet '{name}' exceeds max packet size of {MAX_PACKET_SIZE} bytes by {len(pkt_bytes) - MAX_PACKET_SIZE} bytes.")
                    sys.exit(1)

                pkt_bytes_str = chexdump(pkt_bytes, dump=True)
                length = len([x.strip() for x in pkt_bytes_str.split(",")])

                pkt_bytes_define = PACKET_BYTES_DEFINE_FMT.format(name.upper(), length, pkt_bytes_str)
                self.defines.append(pkt_bytes_define)

    def write_scapy_bytes(self):
        with open(self.output_file, "w") as f:
            f.write(HEADER_BANNER)
            for define in self.defines:
                f.write(define)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <output-header-path>")
        sys.exit(1)
    gen = ScapyHeaderGenerator(sys.argv[1])
    gen.generate_headers()
    gen.write_scapy_bytes()
