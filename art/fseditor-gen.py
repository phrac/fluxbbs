import os
# The full-screen message editor, in the main menu's pink. Run from text/:
#     python3 -I ../art/fseditor-gen.py
# writes FSEDITOR.ANS (the frame) and FSHELP.ANS (its help, /? in the
# editor). The frame: a rule, the header box (to, subject, date, time,
# anonymous, autosig), the text between ~TL (quote color) and ~BL (text
# color) with the status line ~SL on the last row, then the command bar
# (~CM, and line, column and insert at ~LN ~CN ~IN) and the keys.
E='\x1b['
DG=E+'0;1;30m'; RD=E+'0;35m'; BR=E+'0;1;35m'; WH=E+'0;1;37m'; GR=E+'0;37m'
def at(r,c): return E+f'{r};{c}H'
RGB=((255,255,255),(255,85,255),(170,0,170))
def fade_text(t):
    out=''
    for i,ch in enumerate(t):
        x=i/max(1,len(t)-1)*2
        a,b=(RGB[0],RGB[1]) if x<=1 else (RGB[1],RGB[2])
        f=x if x<=1 else x-1
        r,g,bl=(round(a[j]+(b[j]-a[j])*f) for j in range(3))
        out+=E+f'38;2;{r};{g};{bl}m'+ch if ch!=' ' else ' '
    return out
def fade(n,rev):
    cs=[DG if i<n-6 else RD if i<n-1 else BR for i in range(n)]
    if rev: cs=cs[::-1]
    return ''.join(c+'─' for c in cs)
def hline(n):  # bright at the ends, then pink, then gray
    def c(i):
        d=min(i,n-1-i)
        return BR if d==0 else RD if d<3 else DG
    return ''.join(c(i)+'─' for i in range(n))
def rule(title,right=''):
    left=' '+BR+'─'+DG+'[ '+fade_text(title)+' '+DG+']'
    import re
    used=1+1+4+len(title)+(len(re.sub(r'\x1b\[[0-9;]*m','',right))+5 if right else 0)
    s=left+fade(79-used-(1 if right else 0),True)
    if right: s+=DG+'[ '+right+DG+' ]'+BR+'─'
    return s
def label(t): return BR+t[0]+RD+t[1:]

def editor():
    tip=GR+'esc or '+WH+'/'+GR+' on a blank line for the menu'
    s=E+'0m'+E+'2J'+at(1,1)+rule('message editor',tip)
    s+=at(2,1)+RD+'┌'+hline(77)+RD+'┐'
    for r in (3,4): s+=at(r,1)+DG+'│'+at(r,79)+DG+'│'
    s+=at(5,1)+RD+'└'+hline(77)+RD+'┘'
    # Values go where their markers are, in the marker's color; the
    # fields are 40 wide, as the editor cuts them.
    s+=at(3,3)+label('to')+at(3,8)+DG+'»'
    s+=at(4,3)+label('subj')+at(4,8)+DG+'»'
    s+=at(3,52)+label('anon')+at(3,67)+label('sig')
    s+=at(4,52)+label('date')+at(4,67)+label('time')
    s+=at(3,10)+WH+'~RN'+at(4,10)+WH+'~MT'
    s+=at(3,57)+BR+'~AN'+at(3,72)+BR+'~AS'
    s+=at(4,57)+GR+'~DT'+at(4,72)+GR+'~TI'
    # The text: quotes in pink, what's typed in gray.
    s+=at(6,1)+RD+'~TL'+at(19,1)+GR+'~BL'+at(19,1)+'~SL'
    s+=at(20,1)+' '+fade(39,False)+fade(39,True)
    # The command bar, left to right; a marker takes no room, so the
    # cursor skips its value's width after it.
    parts=[(' '+DG+'░'+RD+'▒'+BR+'▓'+WH+' editor '+BR+'▓'+RD+'▒'+DG+'░ '+RD+'( ',18),(BR+'~CM',0,1),(RD+'  ) '+DG+'─'*25,29),
           (DG+'[ '+GR+'line ',7),(WH+'~LN',0,3),(GR+' col ',5),(WH+'~CN',0,2),(GR+' ins ',5),(WH+'~IN',0,3),(DG+' ]'+BR+'─',3)]
    col=1; s+=at(21,1)
    for p in parts:
        s+=p[0]; col+=p[1]
        if len(p)==3: col+=p[2]; s+=at(21,col)
    keys=[('s','save'),('a','abort'),('q','quote'),('t','title'),('l','link'),('n','anon'),('u','sig'),('?','help')]
    s+=at(22,1)+'   '+(DG+' · ').join(BR+k+DG+'/'+GR+w for k,w in keys)
    s+=at(23,1)+E+'0m'
    return s

def helpscreen():
    s=E+'0m'+E+'2J'+at(1,1)+rule('message editor help')
    s+=at(2,1)+RD+'┌'+hline(77)+RD+'┐'
    for r in range(3,22): s+=at(r,1)+DG+'│'+at(r,79)+DG+'│'
    s+=at(22,1)+RD+'└'+hline(77)+RD+'┘'
    def head(r,t): return at(r,3)+DG+'─[ '+fade_text(t)+' '+DG+']'
    def item(r,c,k,w): return at(r,c)+BR+k+at(r,c+9)+GR+w
    s+=head(3,'commands')+at(3,19)+GR+'esc, or / on a blank line, then:'
    cmds=[('s','save the message'),('a','abort it'),('q','quote (replying)'),('t','change the title'),
          ('l','link a file here'),('n','anonymous on/off'),('u','autosig on/off'),('/','type a /'),
          ('?','this help'),('enter','back to writing')]
    for i,(k,w) in enumerate(cmds):
        s+=item(4+i//2,5+(i%2)*38,k,w)
    s+=head(10,'keys')
    keys=[('ctrl-x','save'),('ctrl-b','break the line'),('ctrl-d','delete a character'),('ctrl-k','cut the line'),
          ('ctrl-y','delete the line'),('ctrl-u','paste the cut line'),('ctrl-v','insert on/off'),('ctrl-l','redraw'),
          ('pgup/dn','a page up or down'),('home/end','start or end of line')]
    for i,(k,w) in enumerate(keys):
        s+=item(11+i//2,5+(i%2)*38,k,w)
    # The color codes, each in its own color (|ZA..|ZB shows a code as
    # it is).
    s+=head(16,'colors')+at(16,17)+GR+'type these in the text:'
    dos=[0,4,2,6,1,5,3,7]
    for n in range(16):
        r,c=17+n//8%2,5+(n%8)*9
        col=E+('0;1;30' if n==0 else ('1;' if n>7 else '0;')+str(30+dos[n%8]))+'m'
        s+=at(r,c)+col+'|ZA|'+f'{n:02d}'+'|ZB'
        if n==0: s+=DG
    for n in range(8):
        r,c=19,5+n*9
        s+=at(r,c)+E+f'0;{30 if n else 37};{40+dos[n]}m'+'|ZA|B'+str(n)+'|ZB'+E+'0m'
    s+=at(20,5)+DG+'|ZA|00|ZB-|ZA|15|ZB'+GR+' text, '+DG+'|ZA|B0|ZB-|ZA|B7|ZB'+GR+' background'
    s+=at(23,1)+E+'0m'
    return s

for name,body in (('FSEDITOR.ANS',editor()),('FSHELP.ANS',helpscreen())):
    open(os.path.join(os.getcwd(),name),'wb').write(body.encode('cp437'))
