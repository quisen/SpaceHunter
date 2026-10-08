import struct,sys
d=open('/home/quisen/space-hunter/SpaceMonger.exe','rb').read()
# section .rsrc: va 0x32000 rva -> file 0x2e000
RVA=0x32000; OFF=0x2e000
def u16(o):return struct.unpack_from('<H',d,o)[0]
def u32(o):return struct.unpack_from('<I',d,o)[0]
def name(base,v):
    if v&0x80000000:
        o=base+(v&0x7fffffff); n=u16(o); return d[o+2:o+2+n*2].decode('utf-16le')
    return v
def walk(base,o,path):
    n=u16(o+12)+u16(o+14)
    for i in range(n):
        nm,off=struct.unpack_from('<II',d,o+16+i*8)
        nm=name(base,nm)
        if off&0x80000000: yield from walk(base,base+(off&0x7fffffff),path+[nm])
        else:
            de=base+off; rva,sz=struct.unpack_from('<II',d,de)
            yield path+[nm],rva-RVA+OFF,sz
base=OFF
items=list(walk(base,base,[]))
types={1:'CURSOR',2:'BITMAP',3:'ICON',4:'MENU',5:'DIALOG',6:'STRING',9:'ACCEL',12:'GRPCURSOR',14:'GRPICON',16:'VERSION',24:'MANIFEST',240:'TOOLBAR'}
for p,o,s in items: print(types.get(p[0],p[0]),p[1:],hex(o),s)
import os
os.makedirs('out',exist_ok=True)
for p,o,s in items:
    t=p[0]; b=d[o:o+s]
    if t==2:
        hdr=b'BM'+struct.pack('<IHHI',14+len(b),0,0,14+40+ (struct.unpack_from('<I',b,32)[0] or (1<<struct.unpack_from('<H',b,14)[0]))*4)
        open(f'out/bmp{p[1]}.bmp','wb').write(hdr+b)
        print('bmp',p[1],struct.unpack_from('<iiHH',b,4))
    if t==5: 
        print('DIALOG',p[1], b.decode('utf-16le','ignore') if False else b.replace(b'\0',b'.')[:s].decode('latin1'))
    if t==16:
        print('VERSION', b.decode('utf-16le','ignore').replace('\0',' '))
    if t==3: open(f'out/icon{p[1]}.bin','wb').write(b)
