"""Reproducible, dependency-free Chip Count application mark."""
import math
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).parent

def png(size):
    raw = bytearray()
    for y in range(size):
        raw.append(0)
        for x in range(size):
            xx, yy = (x + .5) / size * 2 - 1, (y + .5) / size * 2 - 1
            d = math.hypot(xx, yy)
            # Rounded graphite tile with a copper casino chip and central bars.
            corner = math.hypot(max(abs(xx)-.68,0),max(abs(yy)-.68,0))
            if corner > .29:
                c=(0,0,0,0)
            else:
                c=(23,27,26,255)
                if .64 < d < .79: c=(218,165,96,255)
                if .665 < d < .765 and abs(math.sin(math.atan2(yy,xx)*6)) < .25: c=(28,34,31,255)
                if .52 < d < .545: c=(146,112,72,255)
                if -.29 < xx < -.15 and -.05 < yy < .27: c=(229,182,115,255)
                if -.065 < xx < .065 and -.23 < yy < .27: c=(229,182,115,255)
                if .15 < xx < .29 and -.37 < yy < .27: c=(229,182,115,255)
            raw.extend(c)
    def chunk(t,b): return struct.pack('>I',len(b))+t+b+struct.pack('>I',zlib.crc32(t+b)&0xffffffff)
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',size,size,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(raw,9))+chunk(b'IEND',b'')

for size,name in [(32,'32x32.png'),(128,'128x128.png'),(256,'128x128@2x.png'),(512,'icon.png')]:
    (ROOT/name).write_bytes(png(size))
iconset = ROOT / 'icon.iconset'
iconset.mkdir(exist_ok=True)
for size in [16,32,128,256,512]:
    (iconset/f'icon_{size}x{size}.png').write_bytes(png(size))
    (iconset/f'icon_{size}x{size}@2x.png').write_bytes(png(size*2))
image = png(256)
(ROOT/'icon.ico').write_bytes(struct.pack('<HHH',0,1,1)+struct.pack('<BBBBHHII',0,0,0,0,1,32,len(image),22)+image)

chunks = b''
for kind,name in [(b'ic07','128x128.png'),(b'ic08','128x128@2x.png'),(b'ic09','icon.png'),(b'ic10','icon.iconset/icon_512x512@2x.png')]:
    image = (ROOT/name).read_bytes()
    chunks += kind + struct.pack('>I',len(image)+8) + image
(ROOT/'icon.icns').write_bytes(b'icns'+struct.pack('>I',len(chunks)+8)+chunks)
