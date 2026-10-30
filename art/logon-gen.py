import importlib.util, os, re, sys
# MATRIX.ANS: the LOGON logo (the board's own art/LOGON.ans, else its
# name in a TheDraw font), a session log above it, the "so you decided
# to proceed" text and the lightbar (menus/MATRIX.toml draws the items at
# ~01..~05, their help at ~HP). Run from text/:
#     python3 -I ../art/logon-gen.py > MATRIX.ANS
E='\x1b['
DG=E+'0;1;30m'; GR=E+'0;37m'; WH=E+'0;1;37m'; RD=E+'0;31m'; BR=E+'0;1;31m'
def at(r,c): return E+f'{r};{c}H'
def fade(t,stops=((255,255,255),(255,85,85),(170,0,0))):
    out=''
    for i,ch in enumerate(t):
        x=i/max(1,len(t)-1)*2
        a,b=(stops[0],stops[1]) if x<=1 else (stops[1],stops[2])
        f=x if x<=1 else x-1
        r,g,bl=(round(a[j]+(b[j]-a[j])*f) for j in range(3))
        out+=E+f'38;2;{r};{g};{bl}m'+ch if ch!=' ' else ' '
    return out
def center(row,t,plain):
    return at(row,1+(80-len(plain))//2)+t
out=[]
# A session log, as if the board were writing it as the caller watches.
LOG=[('carrier detected','ok',GR),
     ('bot check','passed',GR),
     ('warning shown','yes',GR),
     ('warning heeded','no',BR),
     ('caller decision','proceed',WH),
     ('consequences','yours',BR)]
for i,(what,res,c) in enumerate(LOG):
    out.append(at(i+1,5)+DG+'> '+GR+what+' '+DG+'.'*(30-len(what))+' '+c+res)
# On the right: when, where, and a little dread.
X=52
out.append(at(1,X)+RD+'┌'+BR+'─'+DG+'[ '+fade('session')+' '+DG+']'+RD+'─'+DG+'─'*11+RD+'┐')
for i,(l,v) in enumerate([('node','%ND'),('time','%TI'),('term','{10%RT'),('mood','watching')]):
    out.append(at(2+i,X)+DG+'│ '+fade(l)+' '+DG+'.'*(7-len(l))+' '+WH+v+at(2+i,X+25)+DG+'│')
out.append(at(6,X)+RD+'└'+DG+'─'*24+RD+'┘')
for i,ch in enumerate('PROCEED'):
    out.append(at(i+1,2)+(BR if i%2==0 else RD)+ch)
for i,ch in enumerate('ATRISK'):
    out.append(at(i+1,79)+(RD if i%2==0 else BR)+ch)
# Below the logo.
q='so. you decided to proceed.'
out.append(center(17,fade(q),q))
for i,l in enumerate(['the warning was there. you read it. you pressed enter anyway.',
                      "fine. state your business. we'll know if you're lying."]):
    out.append(center(19+i,GR+l,l))
# The lightbar: five items of 14 across, a space apart, then the help line.
W=14; left=1+(80-(5*W+4))//2
out.append(at(22,left)+''.join(at(22,left+i*(W+1))+E+'0m'+f'~{i+1:02d}^{i+1:02d}' for i in range(5)))
out.append(at(24,3)+DG+'~HP'+at(24,79)+'~30'+E+'0m')
# The logo, from a grid of the art (it wraps at 80 and fills columns 1-80):
# columns 2-79 in place, so the first and last columns stay clear.
HERE=os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0,HERE)
import boardname  # noqa: E402
if boardname.art('LOGON.ans'):
    d=open(boardname.art('LOGON.ans'),'rb').read()
    art=d[:d.index(b'\x1a')]
else:
    spec=importlib.util.spec_from_file_location('mainmenu_gen',os.path.join(HERE,'mainmenu-gen.py'))
    mg=importlib.util.module_from_spec(spec); spec.loader.exec_module(mg)
    # Rows 8 to 15, between the session log and the words under it.
    art=b'\r\n'*7+mg.tdf_logo('DAMNEDX.TDF','Damned Red',boardname.NAME,0,{'spacing':0}).replace(b'\n',b'\r\n')
grid={}; row=col=0; fg,bg,bold,blink=7,0,0,0; i=0
while i<len(art):
    m=re.match(rb'\x1b\[([0-9;]*)([A-Za-z])',art[i:])
    if m:
        ps,cmd=m.group(1),m.group(2); i+=m.end()
        if cmd==b'C':
            col+=int(ps or 1)
            while col>=80: col-=80; row+=1
        elif cmd==b'm':
            for v in [int(x) for x in ps.split(b';') if x] or [0]:
                if v==0: fg,bg,bold,blink=7,0,0,0
                elif v==1: bold=1
                elif v==5: blink=1
                elif 30<=v<=37: fg=v-30
                elif 40<=v<=47: bg=v-40
        continue
    c=art[i]; i+=1
    if c==13: col=0
    elif c==10: row+=1
    else:
        grid[(row,col)]=(c,(fg,bg,bold,blink))
        col+=1
        if col>=80: col=0; row+=1
logo=b''
for r in sorted({k[0] for k in grid}):
    cells=[grid.get((r,c)) for c in range(1,79)]
    while cells and (cells[-1] is None or cells[-1][0]==32 and cells[-1][1][1]==0): cells.pop()
    if not cells: continue
    line=at(r+1,2).encode(); cur=None
    for cell in cells:
        ch,a=cell if cell else (32,(7,0,0,0))
        if a!=cur:
            f,b_,bo,bl=a
            line+=('\x1b[0;'+('1;' if bo else '')+('5;' if bl else '')+f'{30+f};{40+b_}m').encode(); cur=a
        line+=bytes([ch])
    logo+=line
sys.stdout.buffer.write(b'\x1b[0m\x1b[2J'+logo+b'\x1b[0m'+''.join(out).encode('cp437'))
