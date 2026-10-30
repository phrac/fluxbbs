import os
# USERSTAT.ANS (your stats, in the logon sequence and on Y): the
# YOURSTATS logo, a rule, and three boxes of %-codes, then a pause.
# A fourth box compares the caller to everyone (the %R/%G rank codes), and
# USERSTAT.FX beside it animates the screen while it waits.
# Run from text/: python3 -I ../art/stats-gen.py > USERSTAT.ANS
E='\x1b['
DG=E+'0;1;30m'; WH=E+'0;1;37m'; GR=E+'0;37m'
ACCENT='35'
RD=E+'0;'+ACCENT+'m'; BR=E+'0;1;'+ACCENT+'m'
def at(r,c): return E+f'{r};{c}H'
def fade_text(t):
    stops=[(255,255,255),(255,85,255),(170,0,170)]
    out=''
    for i,ch in enumerate(t):
        x=i/max(1,len(t)-1)*2
        a,b=(stops[0],stops[1]) if x<=1 else (stops[1],stops[2])
        f=x if x<=1 else x-1
        r,g,bl=(round(a[j]+(b[j]-a[j])*f) for j in range(3))
        out+=E+f'38;2;{r};{g};{bl}m'+ch if ch!=' ' else ' '
    return out
def hline(n):
    return ''.join((BR if min(i,n-1-i)==0 else RD if min(i,n-1-i)<3 else DG)+'─' for i in range(n))
# (box title, [(label, value codes)]); a value's width is cut with {NN.
BOXES=[('you',[('handle','{10%UN'),('level','%UA |08/ |07%SL'),('first call','%FC'),('last call','%LC'),('time left','{06%TL |08min'),('time bank','{06%TB |08min')]),
       ('calls & posts',[('calls','{10%UC'),('today','{10%CT'),('posts','{10%UP'),('email','{10%UE'),('post/call','{10%UZ')]),
       ('files',[('uploads','{10%UL'),('upload kb','{10%UK'),('downloads','{10%DL'),('dl kb','{10%DK'),('points','{10%FP')])]
H=max(len(b[1]) for b in BOXES)
W=25; LEFT=2
d=open(os.path.join(os.path.dirname(os.path.abspath(__file__)),'YOURSTATS.ans'),'rb').read()
art=d[:d.index(b'\x1a')]
body=art.split(b'\x1b[2J',1)[1]
while body.startswith(b'\r\n'): body=body[2:]
body=b'\r\n'+body
rr=body.count(b'\n')+2
out=[]
# The rule, with the title and the caller's name.
out.append(at(rr,2)+''.join((BR if i in (0,77) else RD if min(i,77-i)<4 else DG)+'─' for i in range(78)))
out.append(at(rr,4)+DG+'[ '+fade_text('your stats')+' '+DG+']')
out.append(at(rr,66)+DG+'[ '+WH+'%DT'+' '+DG+']')
for b,(t,items) in enumerate(BOXES):
    x=LEFT+b*(W+1)
    used=len(t)+5
    out.append(at(rr+2,x)+RD+'┌'+BR+'─'+DG+'[ '+fade_text(t)+' '+DG+']'+hline(W-2-used)+RD+'┐')
    for i in range(H):
        y=rr+3+i
        if i<len(items):
            l,v=items[i]
            out.append(at(y,x)+DG+'│ '+fade_text(l)+' '+DG+'.'*(10-len(l))+' '+WH+v+at(y,x+W-1)+DG+'│')
        else:
            out.append(at(y,x)+DG+'│'+at(y,x+W-1)+'│')
    out.append(at(rr+3+H,x)+RD+'└'+hline(W-2)+RD+'┘')
# You against everyone: a bar and a rank for each counter, two a line.
VS=[[('calls','%OC','%RC'),('posts','%OP','%RP')],[('uploads','%OU','%RU'),('downloads','%OD','%RD')],[('points','%OF','%RF'),None]]
y0=rr+4+H
t='you vs. everyone'
out.append(at(y0,LEFT)+RD+'┌'+BR+'─'+DG+'[ '+fade_text(t)+' '+DG+']'+hline(77-5-len(t))+RD+'┐')
for i,pair in enumerate(VS):
    y=y0+1+i
    # The whole row, so nothing from before shows through.
    out.append(at(y,LEFT)+DG+'│'+' '*76+'│')
    for j,cell in enumerate(pair):
        if cell:
            l,bar,rank=cell
            out.append(at(y,LEFT+2+j*38)+fade_text(l)+' '+DG+'.'*(10-len(l))+' '+BR+bar+' '+WH+rank)
out.append(at(y0+4,LEFT)+RD+'└'+hline(76)+RD+'┘')
out.append(at(y0+5,1)+E+'0m|PA')
fx=['# USERSTAT.ANS animated while it waits (fluxbbs [[effect]]).','# Made by art/stats-gen.py; change that, not this.','',
    '[[effect]]','kind = "glint"',f'rows = [1, {rr-1}]','every = 7.0','delay = 1.0','',
    '[[effect]]','kind = "ripple"',f'rows = [{rr+2}, {y0+4}]','every = 7.0','delay = 3.5']
open(os.path.join(os.getcwd(),'USERSTAT.FX'),'w').write('\n'.join(fx)+'\n')
os.sys.stdout.buffer.write(b'\x1b[0m\x1b[2J'+body+b'\x1b[0m'+''.join(out).encode('cp437'))
