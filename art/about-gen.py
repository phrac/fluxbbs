import os, sys
# About flux/bbs (h on the main menu), in the main menu's purple. Run from text/:
#     python3 -I ../art/about-gen.py > ABOUT.ANS
# A board's art/local.py can say it its own way: ABOUT, a list of
# ('head', title), ('text', line) and ('', '') (a blank line).
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import boardname  # noqa: E402
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
ABOUT=[('text','[flux/bbs] is an advanced BBS that began as a port of Iniquity'),
 ('text','1.00a26r5, the Turbo Pascal BBS, to Rust: same menus, ACS and pipe codes,'),
 ('text','rebuilt for SSH and telnet. What it does that the original never did:'),
 ('',''),
 ('head','message boards'),
 ('text','Threads, votes and comments, a hot list, sub-boards, boards you run'),
 ('text','yourself, request boards with points for fills, @mentions, and taking back'),
 ('text','a post you regret.'),
 ('head','networks'),
 ('text','FTN echomail with its own binkp mailer and tosser, and QWK offline mail.'),
 ('head','files'),
 ('text','Browse by folder, an art gallery, pictures and PDFs right in your terminal,'),
 ('text','and the whole textfiles.com archive as file areas.'),
 ('head','doors'),
 ('text','A DOS machine built in runs the classic doors. BBSLink, and our own casino,'),
 ('text','Carrier Lost, tarot and weather.'),
 ('head','and more'),
 ('text','AI chat, a plain text mode, SSH keys, your own time zone, themes, UTF-8 or'),
 ('text','CP437, 24-bit color art, and a bot check that keeps the scripts out.')]
ABOUT=boardname.local('ABOUT', ABOUT)
out=[rule('about flux/bbs'),'']+[head(t) if k=='head' else text(t) if k=='text' else '' for k,t in ABOUT]+[bottom()]
sys.stdout.buffer.write(('\x1b[0m\x1b[2J'+'\x1b[0m\r\n'.join(out)+'\x1b[0m\r\n').encode('cp437'))
