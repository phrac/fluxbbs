import importlib.util, os, sys
# The bar lists: a logo, a rule with live stats, column heads (TOP), and
# matching rows (MID, HIL for the bar) and bottom rule (BOT).
# A logo can be an .ans file, or the menus' TheDraw-font header from
# mainmenu-gen.py: ('menu', name), the same header that menu has.
# Run from text/: python3 -I ../art/listsgen.py   (writes every file)
E='\x1b['
DG=E+'0;1;30m'; WH=E+'0;1;37m'; GR=E+'0;37m'
def at(r,c): return E+f'{r};{c}H'
RGB={'32':((85,255,85),(0,170,0)),'35':((255,85,255),(170,0,170)),'36':((85,255,255),(0,170,170))}
# Each list: logo, accent, title, stats [(label, code, width)],
# columns [(head, col, field, width or None, style)], row end.
# Styles: n number, t title, v value, d dim. Width cuts (and in the bar
# row pads) the field so the bar reaches the next column.
LISTS={
 'FAREA':('FILEAREA.ans','32','file areas',
   [('dls','%TD',7),('ul today','%DU',4),('points','%FP',6)],
   [('#',3,'~AN',4,'n'),('area',8,'~AT',37,'t'),('files',46,'~NF',7,'v'),('newest',54,'~NW',10,'d'),('sponsor',66,'~SP',14,'d')],'\r\n'),
 'MHUB':(('menu', 'message'),'36','message boards',
   [('posts','%TP',7),('today','%DP',4),('yours','%UP',5)],
   [('#',3,'~AN',4,'n'),('board',8,'~AT',16,'t'),('about',25,'~DE',24,'d'),('now',50,'~RD',3,'v'),('new',54,'~NW',4,'v'),('mod',59,'~SP',11,'d'),('latest',71,'~LD',8,'d')],''),
 'MAREA':(('menu', 'message'),'36','message areas',
   [('posts','%TP',7),('today','%DP',4),('yours','%UP',5)],
   [('#',3,'~AN',4,'n'),('area',8,'~AT',36,'t'),('msgs',46,'~NM',7,'v'),('sponsor',54,'~SP',24,'d')],''),
 'MTHRD':('THREADS.ans','36','{30%MA',
   [('messages','%MT',6),('your posts','%MU',5)],
   [('#',3,'~MN',4,'n'),('+/-',8,'~SC',4,'v'),('date',13,'~DT',8,'d'),('author',22,'~SN',12,'d'),('re',35,'~RP',2,'v'),('',38,'~MK',1,'v'),('title',39,'~MT',32,'t'),('new',72,'~NW',7,'v')],'\r\n'),
 'MTREE':('THREADS.ans','36','{30%MA',
   [('messages','%MT',6),('your posts','%MU',5)],
   [('#',3,'~MN',4,'n'),('+/-',8,'~SC',4,'v'),('date',13,'~DT',8,'d'),('',22,'~MK',1,'v'),('who',23,'~TS',22,'t'),('what',46,'~TX',33,'d')],'\r\n'),
 'MHOT':('THREADS.ans','36','hot right now',
   [('posts','%TP',7),('today','%DP',4)],
   [('board',3,'~AT',13,'d'),('+/-',17,'~SC',4,'v'),('date',22,'~DT',8,'d'),('author',31,'~SN',10,'d'),('',42,'~MK',1,'v'),('title',43,'~MT',28,'t'),('new',72,'~NW',7,'v')],'\r\n'),
 'LASTC':('LASTCALLS.ans','35','last callers',
   [('today','%DC',4),('new','%DN',3),('calls','%TC',7)],
   [('#',3,'~CN',5,'n'),('handle',9,'~UN',20,'t'),('from',30,'~LO',18,'d'),('via',49,'~BR',6,'v'),('new',56,'~NU',3,'d'),('date',61,'~DT',8,'d'),('time',71,'~TI',7,'d')],'\r\n'),
 'MSGL':('MSGLIST.ans','36','{30%MA',
   [('messages','%MT',6),('date','%DT',8)],
   [('#',3,'~MN',5,'n'),('from',9,'~SN',21,'t'),('to',31,'~RN',22,'d'),('subject',54,'~MT',25,'v')],'\r\n'),
}
# Lists whose bottom is a prompt bar (the board engine puts the keys and
# the cursor in it), with the name it shows.
BARS={'MHUB':'message boards','MTHRD':'threads','MTREE':'thread tree','MHOT':'hot right now'}
def fade_text(t,accent):
    stops=[(255,255,255)]+list(RGB[accent])
    out=''
    for i,ch in enumerate(t):
        x=i/max(1,len(t)-1)*2
        a,b=(stops[0],stops[1]) if x<=1 else (stops[1],stops[2])
        f=x if x<=1 else x-1
        r,g,bl=(round(a[j]+(b[j]-a[j])*f) for j in range(3))
        out+=E+f'38;2;{r};{g};{bl}m'+ch if ch!=' ' else ' '
    return out
def rule(accent,n=78):
    RD=E+'0;'+accent+'m'; BR=E+'0;1;'+accent+'m'
    cs=[BR if i in (0,n-1) else RD if min(i,n-1-i)<4 else DG for i in range(n)]
    return ''.join(c+'─' for c in cs)
def prompt_bar(name,accent):
    # The keys (~HP) and a prompt bar with the cursor's slot (~PC), as the
    # menus have; the bar is the last row.
    RD=E+'0;'+accent+'m'; BR=E+'0;1;'+accent+'m'
    HIW=E+'0;1;37;4'+accent[1]+'m'
    out=' '+BR+'»'+RD+'»'+DG+'» '+WH+'~HP'+E+'0m\r\n'
    s=' '+DG+'░'+RD+'▒'+BR+'▓'+HIW+' '+name+' '+BR+'▓'+RD+'▒'+DG+'░ '+RD+'('+BR+' ~PC      '+RD+') '
    used=1+3+len(name)+2+3+1+1+1+6+1+1
    stats=DG+'─[ '+WH+'%TL'+GR+' min left'+DG+' ]─[ '+GR+'node '+WH+'%ND'+DG+' ]─[ '+WH+'%TM'+DG+' ]'
    return out+s+DG+'─'*(79-36-used)+'%>'+stats+E+'0m'
def build(prefix):
    logo,accent,title,stats,cols,end=LISTS[prefix]
    RD=E+'0;'+accent+'m'; BR=E+'0;1;'+accent+'m'
    if isinstance(logo, tuple):
        # The menu's own header: its font logo and the art round it.
        spec=importlib.util.spec_from_file_location('mainmenu_gen', os.path.join(os.path.dirname(os.path.abspath(__file__)),'mainmenu-gen.py'))
        mg=importlib.util.module_from_spec(spec); spec.loader.exec_module(mg)
        mlogo,maccent=mg.LOGOS[logo[1]]
        ink=mg.Ink(maccent)
        head=mg.logo_head(logo[1],mlogo)
        extras={'message':lambda: mg.message_head_extras(ink),'file':lambda: mg.file_head_extras(ink,head.count(b'\n'))}.get(logo[1])
        body=head.split(b'\x1b[2J',1)[1]
        rows=body.count(b'\n')
        if extras:
            body+=(extras()+E+'0m'+at(rows+1,1)).encode('cp437')
    else:
        d=open(os.path.join(os.path.dirname(os.path.abspath(__file__)),logo),'rb').read()
        art=d[:d.index(b'\x1a')]
        # Start the logo on the first row.
        body=art.split(b'\x1b[2J',1)[1]
        while body.startswith(b'\r\n'): body=body[2:]
        body=b'\r\n'+body
        rows=body.count(b'\n')+1
    rr=rows+1
    top=[at(rr,2)+rule(accent)]
    # The title: a %-code (an area name) is shown plain, cut to fit.
    t=BR+title if title.startswith('{') else fade_text(title,accent)
    top.append(at(rr,4)+DG+'[ '+t+' '+DG+']')
    s=DG+'[ '+(' '+BR+'·'+' ').join(fade_text(l,accent)+' '+WH+'{%02d'%w+c for l,c,w in stats)+' '+DG+']'
    slen=4+sum(len(l)+1+w for l,c,w in stats)+3*(len(stats)-1)
    top.append(at(rr,79-slen)+s)
    top.append(at(rr+1,1)+''.join(at(rr+1,c)+fade_text(h,accent) for h,c,_,_,_ in cols))
    top.append(at(rr+2,1)+E+'0m')
    style={'n':RD,'t':GR,'v':BR,'d':DG}
    hstyle={'n':BR,'t':WH,'v':WH,'d':GR}
    mid=E+'255D'+E+'0m'+''.join(f'@{c:02d}'+style[s]+(f'{{{w:02d}' if w else '')+f for _,c,f,w,s in cols)+E+'0m'+end
    hil=E+'255D'+E+'0m'+BR+'\x10'+''.join(f'@{c:02d}'+hstyle[s]+(f'{{{w:02d}' if w else '')+f for _,c,f,w,s in cols)+E+'0m'+end
    bot=E+'255D'+E+'0m '+rule(accent)+E+'0m\r\n'
    if prefix in BARS:
        bot+=prompt_bar(BARS[prefix],accent)
    head=b'\x1b[0m\x1b[2J'+body+b'\x1b[0m'
    return {'TOP':head+''.join(top).encode('cp437'),'MID':mid.encode('cp437'),'HIL':hil.encode('cp437'),'BOT':bot.encode('cp437')}
out=sys.argv[1] if len(sys.argv)>1 else '.'
for p in LISTS:
    for part,data in build(p).items():
        open(os.path.join(out,p+part+'.ANS'),'wb').write(data)
# The message reader's bottom: the same bar, the keys above it.
open(os.path.join(out,'MSGRBAR.ANS'),'wb').write((E+'0m'+prompt_bar('reading','36')).encode('cp437'))
