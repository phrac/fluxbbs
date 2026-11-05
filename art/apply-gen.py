import importlib.util, os, sys
# APPLY.ANS: shown before the new-user application (`applyToBBS`), in the
# about screen's purple. Run from text/:
#     python3 -I ../art/apply-gen.py > APPLY.ANS
# A board's art/local.py can say it its own way: APPLY, a list of
# ('head', title), ('text', line) and ('', '') (a blank line); and
# APPLY_LOGO, the word over it and its TheDraw font (file, face).
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import boardname  # noqa: E402
spec = importlib.util.spec_from_file_location('mm', os.path.join(HERE, 'mainmenu-gen.py'))
mm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mm)
E='\x1b['
DG=E+'0;1;30m'; GR=E+'0;37m'; WH=E+'0;1;37m'
accent='35'
RD=E+'0;'+accent+'m'; BR=E+'0;1;'+accent+'m'
STOPS=[(255,255,255),(255,85,255),(170,0,170)]
def fade_text(t):
    out=''
    for i,ch in enumerate(t):
        x=i/max(1,len(t)-1)*2
        a,b=(STOPS[0],STOPS[1]) if x<=1 else (STOPS[1],STOPS[2])
        f=x if x<=1 else x-1
        r,g,bl=(round(a[j]+(b[j]-a[j])*f) for j in range(3))
        out+=E+f'38;2;{r};{g};{bl}m'+ch if ch!=' ' else ' '
    return out
def fade(n,rev):
    cs=[DG if i<n-6 else RD if i<n-1 else BR for i in range(n)]
    if rev: cs=cs[::-1]
    return ''.join(c+'─' for c in cs)
def rule(t):
    side=(78-len(t)-6)//2; rest=78-len(t)-6-side
    return ' '+fade(side,False)+DG+' [ '+fade_text(t)+' '+DG+']'+' '+fade(rest,True)
def bottom():
    return ' '+''.join((BR if i in (0,77) else RD if min(i,77-i)<6 else DG)+'─' for i in range(78))
def head(t):
    return ' '+BR+'» '+fade_text(t)
# A line of text; words in [brackets] are white.
def text(t):
    plain=t.replace('[','').replace(']','')
    assert len(plain)<=75, plain
    return '   '+GR+t.replace('[',WH).replace(']',GR)
APPLY=[('head','what we ask'),
 ('text','A [handle], the name everyone here knows you by, and a [password]. The'),
 ('text','SysOp may ask a little more; anything not asked is left blank.'),
 ('text','Press [Enter] at the handle to change your mind and go back.'),
 ('head','new callers'),
 ('text','You start with the basics. Full access comes once the SysOp, or the'),
 ('text','board, has looked you over.'),
 ('head','the rules'),
 ('text','Be decent. One account each. No flooding, no spam, no scripts.'),
 ('head','your password'),
 ('text','Kept only as an [argon2] hash: nobody here can read it, the SysOp included.')]
APPLY=boardname.local('APPLY', APPLY)
word, file, face = boardname.local('APPLY_LOGO', ('apply', 'BODYCNTX.TDF', 'Body Count P'))
logo = mm.tdf_logo(file, face, word, 0).decode('cp437').rstrip('\n').split('\n')
out=logo+['',rule('apply to join '+boardname.NAME.lower())]+[head(t) if k=='head' else text(t) if k=='text' else '' for k,t in APPLY]+[bottom()]
sys.stdout.buffer.write(('\x1b[0m\x1b[2J'+'\x1b[0m\r\n'.join(out)+'\x1b[0m\r\n').encode('cp437'))
