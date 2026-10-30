import sys
# Multinode chat, in the multinode menu's red. Run from text/:
#     python3 -I ../art/nodechat-gen.py screen > NODECHAT.ANS
#     python3 -I ../art/nodechat-gen.py strings   (the MNC* lines for strings.toml)
# The screen: a rule with the help under it at the top, the conversation
# between ~TL and ~BL, the prompt bar, and the input line (~SI to ~EI) on
# row 24.
E='\x1b['
DG=E+'0;1;30m'; RD=E+'0;31m'; BR=E+'0;1;31m'; WH=E+'0;1;37m'; GR=E+'0;37m'
def at(r,c): return E+f'{r};{c}H'
RGB=((255,255,255),(255,85,85),(170,0,0))
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
def screen():
    title='multinode chat'
    # The rule, with who and where on its right (%ND node, %UN handle,
    # cut to 12).
    left=' '+BR+'─'+DG+'[ '+fade_text(title)+' '+DG+']'
    used=1+1+4+len(title)
    right=DG+'[ '+GR+'node '+WH+'%ND'+DG+' · '+WH+'{12%UN'+DG+' ]'+BR+'─'
    rule=left+fade(79-used-26,True)+right
    keys=['/who','/me','/msg','/page','/help']
    help_=' '+GR+'type, then '+WH+'enter'+GR+' to talk'+DG+' · '+(DG+' · ').join(BR+k for k in keys)+DG+' · '+BR+'esc'+GR+' leaves'
    bar=(' '+DG+'░'+RD+'▒'+BR+'▓'+WH+' chat '+BR+'▓'+RD+'▒'+DG+'░ '+DG+'─'*30+'%>'
         +DG+'─[ '+WH+'%TL'+GR+' min left'+DG+' ]─[ '+GR+'node '+WH+'%ND'+DG+' ]─[ '+WH+'%TM'+DG+' ]')
    s=E+'0m'+E+'2J'+at(1,1)+rule+E+'0m'+at(2,1)+help_+E+'0m'
    s+=at(4,1)+'~TL'+at(22,1)+'~BL'
    s+=at(23,1)+bar+E+'0m'
    s+=at(24,1)+' '+BR+'»'+RD+'»'+DG+'» '+E+'0m'+'~SI'+at(24,79)+'~EI'
    return s.encode('cp437')
def strings():
    S={
     'MNCyouspeak':'|08(|15%s1|08) |07%s2',
     'MNCtheyspeak':'|08(|12%s1|08) |07%s2',
     'MNCenterchat':'|12» |15%s1 |07is here',
     'MNCquitchat':'|04« |08%s1 left',
     'MNCaction':'|12* |15%s1 |07%s2',
     'MNCprivate':'|08(|13%s1 |05whispers|08) |07%s2',
     'MNCprivateSent':'|08(|05to |13%s1|08) |07%s2',
     'MNCinChat':'|08in chat|04: |15%s1',
     'MNCwho':'|08node |12%s1|08: |15%s2 |08- |07%s3',
     'MNCnotFound':'|04nobody by that name|08.',
     'MNChint':'|08type, |15enter|08 talks. |12/help|08 for more, |12esc|08 leaves.',
    }
    for k,v in S.items(): print(f'{k} = "{v}"')
if __name__=='__main__':
    if (sys.argv[1:] or ['screen'])[0]=='strings': strings()
    else: sys.stdout.buffer.write(screen())
