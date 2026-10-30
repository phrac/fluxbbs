import os, random, sys
# BOTCHECK.ANS: the picture bot check's screen. The board draws three big
# characters in color into the field marked ~BC (40x9) and takes the
# answer at ~BP. Run from text/: python3 -I ../art/botcheck-gen.py > BOTCHECK.ANS
E='\x1b['
DG=E+'0;1;30m'; GR=E+'0;37m'; WH=E+'0;1;37m'; MG=E+'0;35m'; BM=E+'0;1;35m'; BR=E+'0;1;31m'
def at(r,c): return E+f'{r};{c}H'
def fade(t,stops=((255,255,255),(255,85,255),(170,0,170))):
    out=''
    for i,ch in enumerate(t):
        x=i/max(1,len(t)-1)*2
        a,b=(stops[0],stops[1]) if x<=1 else (stops[1],stops[2])
        f=x if x<=1 else x-1
        r,g,bl=(round(a[j]+(b[j]-a[j])*f) for j in range(3))
        out+=E+f'38;2;{r};{g};{bl}m'+ch if ch!=' ' else ' '
    return out
def center(row,t,plain): return at(row,1+(80-len(plain))//2)+t
def rule(n,rev=False):
    cs=[DG]*(n-6)+[MG]*5+[BM]
    if rev: cs=cs[::-1]
    return ''.join(c+'─' for c in cs)
out=[]
# The top: a rule with the intercept tag, and a line of noise under it.
tag='[ intercept ]'
out.append(at(1,2)+rule(30)+DG+' [ '+fade('intercept')+' '+DG+'] '+rule(33,True))
rnd=random.Random(2026)
noise=''.join(DG+rnd.choice('ABCDEFGHJKLMNPQRSTUVWXYZ0123456789')+(' ' if i%4==3 else '') for i in range(48))
out.append(at(2,(80-60)//2+1)+noise)
q='something is calling in. we need to know what.'
out.append(center(4,fade(q),q))
# The field's frame: the field is 40x9 at row 7, column 21.
FX,FY,FW,FH=21,7,40,9
title='prove you are not a machine'
top=MG+'┌'+BM+'─'+DG+'[ '+fade(title)+' '+DG+']'
used=1+1+2+len(title)+2
top+=DG+'─'*(FW+2-used-2)+MG+'─┐'
out.append(at(FY-1,FX-1)+top)
for r in range(FH):
    out.append(at(FY+r,FX-1)+(MG if r in (0,FH-1) else DG)+'│'+at(FY+r,FX+FW)+(MG if r in (0,FH-1) else DG)+'│')
out.append(at(FY+FH,FX-1)+MG+'└─'+DG+'─'*(FW-2)+MG+'─┘')
out.append(at(FY,FX)+'~BC')
# Down the sides.
for i,ch in enumerate('HUMAN?'):
    out.append(at(FY+1+i,FX-5)+(BM if i%2==0 else MG)+ch)
for i,ch in enumerate('VERIFY'):
    out.append(at(FY+1+i,FX+FW+4)+(MG if i%2==0 else BM)+ch)
# The trace, small, to the right of the title.
out.append(at(4,66)+DG+'node '+WH+'%ND'+at(5,66)+DG+'time '+WH+'%TI')
# Under the field: how to read it, then the answer.
lines=[('the shapes are made of color, not letters.',GR),
       ('machines see noise. you see three characters.',GR)]
for i,(l,c) in enumerate(lines):
    out.append(center(FY+FH+2+i,c+l,l))
out.append(at(FY+FH+5,FX)+'~BP')
out.append(at(24,2)+rule(77))
sys.stdout.buffer.write(('\x1b[0m\x1b[2J'+''.join(out)+E+'0m').encode('cp437'))
