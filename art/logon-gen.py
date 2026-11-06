import importlib.util, os, re, sys
# MATRIX.ANS: the LOGON logo (the board's own art: art/local.py's
# LOGON_ART, else its art/LOGON.ans, else its name in a TheDraw font), a
# session log above it, the "so you decided to proceed" text and the
# lightbar (menus/MATRIX.toml draws the items at ~01..~05, their help at
# ~HP). A logo taller than 8 rows takes the top of the screen instead of
# the session log. LOGON_ROWS (first, last) takes only those rows of the
# art; MATRIX_COLORS colors it (boardname.MATRIX_COLORS). Run from text/:
#     python3 -I ../art/logon-gen.py > MATRIX.ANS
HERE=os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0,HERE)
import ansgrid  # noqa: E402
import boardname  # noqa: E402
C=boardname.matrix_colors()
E='\x1b['
DG=E+'0;1;30m'; GR=E+'0;37m'; WH=E+'0;1;37m'; RD=E+'0;'+C['dim']+'m'; BR=E+'0;'+C['bright']+'m'
def at(r,c): return E+f'{r};{c}H'
def fade(t,stops=C['stops']):
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

# The logo, from a grid of the art (it wraps at 80 and fills columns 1-80;
# 16, 256 and 24-bit colors): columns 2-79, so the first and last
# columns stay clear.
name=boardname.local('LOGON_ART','LOGON.ans')
if boardname.art(name):
    d=open(boardname.art(name),'rb').read()
    art=d
else:
    spec=importlib.util.spec_from_file_location('mainmenu_gen',os.path.join(HERE,'mainmenu-gen.py'))
    mg=importlib.util.module_from_spec(spec); spec.loader.exec_module(mg)
    # Rows 8 to 15, between the session log and the words under it.
    art=b'\r\n'*7+mg.tdf_logo('DAMNEDX.TDF','Damned Red',boardname.NAME,0,{'spacing':0}).replace(b'\n',b'\r\n')
grid=ansgrid.parse(art)
rows=sorted({k[0] for k in grid})
first,last=boardname.local('LOGON_ROWS',(1,999))
rows=[r for r in rows if first-1<=r<=last-1]
# A tall logo starts at row 2; the stock one keeps the rows it came with.
tall=len(rows)>8
shift=(1-rows[0]) if tall and rows else 0
logo=b''.join(ansgrid.line(grid,r,1,78,2,r+1+shift) for r in rows)

out=[]
if not tall:
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
sys.stdout.buffer.write(b'\x1b[0m\x1b[2J'+logo+b'\x1b[0m'+''.join(out).encode('cp437'))
