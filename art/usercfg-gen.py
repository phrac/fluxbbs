import sys
# USERCFG.ANS (user config, C on the main menu), in the menus' style: the
# STOCK header (|SA), the rule, and two boxes. Each field is a marker
# where the board writes its value: ~XK is field X on key K (DL location
# on l, and so on; P is the SSH keys, Q the plan, R the time zone, S
# plain text or ANSI, T code's background, U and V the code and Markdown
# color themes, W menus animating or not, X the screen saver, Z the
# editor's keys, standard or vim), ~*A the theme's
# author and ~** the command prompt.
# Run from text/: python3 -I ../art/usercfg-gen.py > USERCFG.ANS
E='\x1b['
DG=E+'0;1;30m'; WH=E+'0;1;37m'; GR=E+'0;37m'
ACCENT='31'
RD=E+'0;'+ACCENT+'m'; BR=E+'0;1;'+ACCENT+'m'
RGB=((255,85,85),(170,0,0))
def fade_text(t):
    stops=[(255,255,255)]+list(RGB)
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
def fade(n,rev):
    cs=[DG if i<n-6 else RD if i<n-1 else BR for i in range(n)]
    if rev: cs=cs[::-1]
    return ''.join(c+'─' for c in cs)
# (title, width, label width, [(key, label, marker)]); the marker is
# where the value goes (values run to 30 characters about you, 10 in
# settings).
BOXES=[('about you',47,12,[('l','location','~DL'),('a','address','~EA'),('u','usernote','~FU'),('n','phone','~BN'),
                        ('z','zip code','~OZ'),('p','profile','~QP'),('k','ssh keys','~PK'),('t','theme','~NT'),(None,'by','~*A'),('r','code theme','~UR'),('m','md theme','~VM'),('i','editor keys','~ZI')]),
       ('settings',30,12,[('s','screen len','~GS'),('e','emulation','~ME'),('q','autoquote','~LQ'),('y','yes/no bars','~HY'),
                       ('x','expert mode','~IX'),('h','hot keys','~JH'),('w','pausing','~KW'),('c','time zone','~RC'),('b','text mode','~SB'),('g','code bg','~TG'),('v','animations','~WV'),('d','screensaver','~XD')])]
H=max(len(b[3]) for b in BOXES)
def top(t,w):
    head=BR+'─'+DG+'[ '+fade_text(t)+' '+DG+']'
    return RD+'┌'+head+hline(w-2-(len(t)+5))+RD+'┐'
def bot(w, label='', shown=0):
    # A box's bottom, with keys set into it (`shown` columns of them as
    # they show; markers show as nothing).
    if not label:
        return RD+'└'+hline(w-2)+RD+'┘'
    head=BR+'─'+DG+'[ '+label+' '+DG+']'
    return RD+'└'+head+hline(w-2-(shown+5))+RD+'┘'
def row(item,w,LABEL):
    if item is None: return DG+'│'+' '*(w-2)+DG+'│'
    k,l,m=item
    key=(' '+DG+'('+BR+k+DG+') ') if k else '     '
    body=key+fade_text(l)+' '*(LABEL-len(l))+WH+m
    used=5+LABEL
    return DG+'│'+body+' '*(w-2-used)+DG+'│'
# The keys sit in the boxes' bottoms, so the screen keeps to 24 rows
# under the STOCK header.
KEYS=[(DG+'('+BR+'enter'+DG+') '+fade_text('save')+'  '+DG+'('+BR+'esc'+DG+') '+fade_text('abort'), 25),
      (DG+'command'+RD+': '+WH+'~**', 9)]
cols=[[top(t,w)]+[row(items[i] if i<len(items) else None,w,lw) for i in range(H)]+[bot(w,*KEYS[n])] for n,(t,w,lw,items) in enumerate(BOXES)]
out=[]
t='user config'; side=(78-len(t)-6)//2; rest=78-len(t)-6-side
out.append(' '+fade(side,False)+DG+' [ '+fade_text(t)+' '+DG+']'+' '+fade(rest,True))
for i in range(H+2):
    out.append(' '+' '.join(c[i] for c in cols))
sys.stdout.buffer.write(('\x1b[0m\x1b[2J|SA'+'\x1b[0m\r\n'.join(out)+'\x1b[0m').encode('cp437'))
