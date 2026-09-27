"""Write a minimal 32x32 icon.ico (Tauri requires one on Windows). Generated, not committed."""
import struct
from pathlib import Path

w = h = 32
px = bytes([0x5A, 0x4A, 0x3A, 0xFF]) * (w * h)  # BGRA, solid slate
mask = bytes((w // 8) * h)
bih = struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, len(px) + len(mask), 0, 0, 0, 0)
img = bih + px + mask
ico = struct.pack("<HHH", 0, 1, 1) + struct.pack("<BBBBHHII", w, h, 0, 0, 1, 32, len(img), 22) + img
Path(__file__).with_name("icon.ico").write_bytes(ico)
