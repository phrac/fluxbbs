import os, sys
# LISTFILE.ANS / GALLERY.ANS: the list screens, with the FILELIST / GALLERY
# logo and a stats box in its empty upper right.
# Run from text/: python3 -I ../art/listgen.py file > LISTFILE.ANS
#                 python3 -I ../art/listgen.py gallery > GALLERY.ANS
E='\x1b['
DG=E+'0;1;30m'; WH=E+'0;1;37m'; GR=E+'0;37m'
def at(r,c): return E+f'{r};{c}H'
# logo, accent, box title, rows (label, value), first list row, column heads
SCREENS={
 'file':('FILELIST.ans','32','this area',
         [('files','~NA'),('folders','~NF'),('size','~SZ'),('your dls','%DL'),('points','%FP')],
         ('name','size','description')),
 'gallery':('GALLERY.ans','31','in here',
         [('files','~NA'),('folders','~NF'),('art size','~SZ'),('your dls','%DL'),('date','%DT')],
         ('piece','size','title / artist')),
}
name=sys.argv[1] if len(sys.argv)>1 else 'file'
logo,accent,btitle,rows,heads=SCREENS[name]
RD=E+'0;'+accent+'m'; BR=E+'0;1;'+accent+'m'
RGB={'31':((255,85,85),(170,0,0)),'32':((85,255,85),(0,170,0))}
def fade_text(t):
    stops=[(255,255,255)]+list(RGB[accent])
    out=''
    for i,ch in enumerate(t):
        x=i/max(1,len(t)-1)*2
        a,b=(stops[0],stops[1]) if x<=1 else (stops[1],stops[2])
        f=x if x<=1 else x-1
        r,g,bl=(round(a[j]+(b[j]-a[j])*f) for j in range(3))
        out+=E+f'38;2;{r};{g};{bl}m'+ch if ch!=' ' else ' '
    return out
def hline(n):
    def c(i):
        d=min(i,n-1-i)
        return BR if d==0 else RD if d<3 else DG
    return ''.join(c(i)+'─' for i in range(n))
d=open(os.path.join(os.path.dirname(os.path.abspath(__file__)),logo),'rb').read()
art=d[:d.index(b'\x1a')]
logo_rows=art.count(b'\n')+1
out=[]
# The box: columns 51-78, rows 1-7. Values first, then the right border,
# so a long value can't push it out.
X,W=51,28
out.append(at(1,X)+RD+'┌'+BR+'─'+DG+'[ '+fade_text(btitle)+' '+DG+']'+hline(W-2-(len(btitle)+5))+RD+'┐')
for i,(label,code) in enumerate(rows):
    lw=10
    out.append(at(2+i,X)+DG+'│ '+fade_text(label)+' '+DG+'.'*(lw-len(label))+' '+WH+code)
    out.append(at(2+i,X+W-1)+DG+'│')
out.append(at(2+len(rows),X)+RD+'└'+hline(W-2)+RD+'┘')
# The rule under the logo, with where we are on it.
rr=max(logo_rows,2+len(rows))+1
def fade(n,rev):
    cs=[DG if i<n-6 else RD if i<n-1 else BR for i in range(n)]
    if rev: cs=cs[::-1]
    return ''.join(c+'─' for c in cs)
out.append(at(rr,2)+fade(4,False)+fade(73,True))
out.append(at(rr,7)+DG+'[ '+BR+'~FA'+' '+DG+']')
# column heads, then the list and the stock lister's markers: names get
# 16 columns (most fit; one cut short ends in ~) so descriptions get 50
NAME,SIZE,DESC=3,20,29
out.append(at(rr+1,NAME)+fade_text(heads[0])+at(rr+1,SIZE)+fade_text(heads[1])+at(rr+1,DESC)+fade_text(heads[2]))
top=rr+2
out.append(at(top,1)+'~TL'+at(20,1)+'~BL'+GR+at(top,NAME)+'~FN'+at(top,SIZE)+'~FS'+at(top,DESC)+'~FD')
out.append(at(21,2)+DG+'~S─'+at(21,79)+BR+'~E═')
# The menu, the info line and, on the caller's last row, the prompt bar
# the menus have (255 is past any bottom, and terminals stop there), with
# the cursor waiting in its slot (~PC).
HIW=E+'0;1;37;4'+accent[1]+'m'
bartitle={'file':'file list','gallery':'art gallery'}[name]
bar=' '+DG+'░'+RD+'▒'+BR+'▓'+HIW+' '+bartitle+' '+BR+'▓'+RD+'▒'+DG+'░ '+RD+'('+BR+' ~PC  '+RD+') '
used=1+3+len(bartitle)+2+3+1+1+1+2+1+1
stats=DG+'─[ '+WH+'%TL'+GR+' min left'+DG+' ]─[ '+GR+'node '+WH+'%ND'+DG+' ]─[ '+WH+'%TM'+DG+' ]'
out.append(E+'255;1H'+E+'2A'+GR+'~PM')
out.append(E+'255;1H'+E+'A'+' '+BR+'»'+RD+'»'+DG+'» '+GR+'~PI')
out.append(E+'255;1H'+bar+DG+'─'*(79-36-used)+'%>'+stats+E+'0m')
bg={'31':'41','32':'42'}[accent]
out.append(at(25,1)+E+'0;37m~C1'+DG+'~C2'+BR+'~C3'+E+f'0;1;37;{bg}m~C4'+E+f'0;37;{bg}m~C5'+E+f'0;1;33;{bg}m~C6'+E+'0m')
sys.stdout.buffer.write(b'\x1b[0m'+art+b'\x1b[0m'+''.join(out).encode('cp437'))
