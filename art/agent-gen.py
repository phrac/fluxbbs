import os, sys
# AI chat, in the AGENT logo's cyan. Run from text/:
#     python3 -I ../art/agent-gen.py screen > AICHAT.ANS
#     python3 -I ../art/agent-gen.py strings   (the Ai* lines for strings.toml)
E='\x1b['
def rgb(c): return E+'38;2;%d;%d;%dm'%c
DIM=(85,85,85); TXT=(170,170,170); WHT=(255,255,255); CY=(85,255,255); CYD=(0,170,170); INK=(150,235,235)
def fade(t,stops=(WHT,CY,CYD)):
    out=''
    for i,ch in enumerate(t):
        x=i/max(1,len(t)-1)*2
        a,b=(stops[0],stops[1]) if x<=1 else (stops[1],stops[2])
        f=x if x<=1 else x-1
        out+=rgb(tuple(round(a[j]+(b[j]-a[j])*f) for j in range(3)))+ch if ch!=' ' else ' '
    return out
def screen():
    d=open(os.path.join(os.path.dirname(os.path.abspath(__file__)),'AGENT.ans'),'rb').read()
    art=d[:d.index(b'\x1a')]
    # The rule: bright at the ends, cyan, then gray; a tag over each end.
    n=78
    rule=' '+''.join(rgb(CY if i in (0,n-1) else CYD if min(i,n-1-i)<5 else DIM)+'─' for i in range(n))
    left=rgb(DIM)+'[ '+fade('agent chat')+' '+rgb(DIM)+']'
    right=rgb(DIM)+'[ '+fade('it forgets you when you leave')+' '+rgb(DIM)+']'
    rlen=4+len('it forgets you when you leave')
    tags='\r'+E+'3C'+left+'\r'+E+f'{79-rlen-2}C'+right
    help=(' '+rgb(TXT)+'type a message and press '+rgb(WHT)+'enter'+rgb(TXT)+'. '
          +rgb(CY)+'/help'+rgb(TXT)+' explains, '+rgb(CY)+'/quit'+rgb(TXT)+' leaves.')
    body=rule+tags+E+'0m\r\n'+help+E+'0m\r\n\r\n'
    return b'\x1b[0m'+art+b'\x1b[0m\r\n'+body.encode('cp437')
def strings():
    def c(col): return rgb(col)
    S={
     'AiYou': c(DIM)+'('+c(WHT)+'%S1'+c(DIM)+') '+c(TXT),
     'AiName': c(DIM)+'('+c(CY)+'%S1'+c(DIM)+') '+c(INK),
     'AiSearching': c(DIM)+'  ['+c(CYD)+'searching'+c(DIM)+': '+c(WHT)+'%S1'+c(DIM)+']',
     'AiReading': c(DIM)+'  ['+c(CYD)+'reading'+c(DIM)+': '+c(WHT)+'%S1'+c(DIM)+']',
     'AiLooking': c(DIM)+'  ['+c(CYD)+'looking'+c(DIM)+': '+c(WHT)+'%S1'+c(DIM)+']',
     'AiBar': ' '+c(DIM)+'['+c(CY)+'/new'+c(DIM)+'] ['+c(CY)+'/quit'+c(DIM)+'] ['+c(CY)+'/help'+c(DIM)+'] - %S1 %S2',
     'AiStats': c(DIM)+'  ['+c(TXT)+'%S1 tokens'+c(DIM)+' · '+c(TXT)+'%S2 thinking'+c(DIM)+' · '+c(CY)+'%S3'+c(TXT)+' tok/s'+c(DIM)+']',
     'AiMore': ' '+c(DIM)+'-- '+c(CY)+'more'+c(DIM)+': '+c(TXT)+'any key goes on, '+c(CY)+'pgup'+c(TXT)+' looks back, '+c(CY)+'esc'+c(TXT)+' no more pauses '+c(DIM)+'--',
     'AiBack': ' '+c(DIM)+'['+c(TXT)+'looking back '+c(CY)+'%S1'+c(TXT)+' lines'+c(DIM)+'] '+c(CY)+'pgup'+c(DIM)+'/'+c(CY)+'pgdn'+c(TXT)+' scroll, '+c(CY)+'end'+c(TXT)+' or type to come back',
     'AiContext': c(DIM)+'· '+c(TXT)+'context '+c(CY)+'%S1'+c(DIM)+'/'+c(TXT)+'%S2 '+c(DIM)+'('+c(CY)+'%S3%'+c(DIM)+')',
     'AiLeft': c(CY)+'%S1'+c(TXT)+' messages left today',
     'AiUnlimited': c(TXT)+'no daily limit',
     'AiHelp1': c(CY)+' /new'+c(TXT)+'   forget this conversation and start over',
     'AiHelp2': c(CY)+' /quit'+c(TXT)+'  leave (so do '+c(CY)+'esc'+c(TXT)+' and '+c(CY)+'q'+c(TXT)+'); the conversation is forgotten',
     'AiHelp3': c(CY)+' /help'+c(TXT)+'  this list',
     'AiHelp4': c(TXT)+' It can search the web when it needs to. Long messages wrap as you type.',
     'AiError': c(DIM)+"The AI didn't answer: "+c(CYD)+'%S1',
     'AiLimit': c(TXT)+"That's your "+c(CY)+'%S1'+c(TXT)+' messages for today; come back tomorrow.',
    }
    return {k:v.replace('\x1b','\\u001b') for k,v in S.items()}
if sys.argv[1]=='screen':
    sys.stdout.buffer.write(screen())
else:
    for k,v in strings().items(): print(f'{k} = "{v}"')
