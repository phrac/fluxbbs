import sys
# The message screens, in the message menu's cyan. Run from text/:
#     python3 -I ../art/msgguide-gen.py > MSGGUIDE.ANS        (h on the message menu: the keys)
#     python3 -I ../art/msgguide-gen.py news > MSGNEWS.ANS    (at logon: a short overview)
E='\x1b['
DG=E+'0;1;30m'; GR=E+'0;37m'; WH=E+'0;1;37m'
accent='36'
RD=E+'0;'+accent+'m'; BR=E+'0;1;'+accent+'m'
STOPS=[(255,255,255),(85,255,255),(0,170,170)]
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
# A key and what it does, in two columns.
def keys(*pairs):
    return '   '+''.join(WH+k.ljust(12)+GR+what.ljust(26) for k,what in pairs).rstrip()
def text(t):
    return '   '+GR+t
if len(sys.argv)>1 and sys.argv[1]=='news':
    out=[rule('message boards'),'',
     text('You can now vote posts up or down and leave a short comment under them.'),
     text('Threads people are voting on show up at the top of the board list.'),
     '',
     head('reading'),
     text('Press '+WH+'m'+GR+' on the main menu for the boards. Pick a board, then a thread.'),
     text('Threads with new posts have a '+WH+'*'+GR+', and '+WH+'n'+GR+' reads everything new in one go.'),
     keys(('arrows','move'),('enter','open')),
     keys(('esc','go back'),('r','reply')),
     '',
     head('votes and comments'),
     text('While reading a post, press '+WH+'x'+GR+', then:'),
     keys(('+','vote it up'),('-','vote it down')),
     keys(('>','leave a comment'),('r','comment, quicker')),
     text('Your vote and comment appear under the post with your name. You get one'),
     text('vote per post and can\'t vote on your own. Each thread shows its score,'),
     text('and '+WH+'s'+GR+' sorts by latest, hot or top score.'),
     '',
     text(WH+'h'+GR+' anywhere in the boards lists all the keys.'),
     '',bottom()]
else:
    out=[rule('message board keys'),
     head('everywhere'),
     keys(('arrows','move'),('enter','open')),
     keys(('esc','go back'),('q','leave the boards')),
     head('board list'),
     keys(('n','read all new'),('p','post')),
     keys(('f','find posts'),('s','sort: busiest')),
     keys(('m','more options'),('c','make your own board')),
     keys(('a','add to my boards'),('*','go to my boards')),
     head('thread list'),
     keys(('n','new in this board'),('s','sort: latest, hot, top')),
     keys(('t','show one tag'),('o','moderate: pin, tags, bans')),
     keys(('v','the thread as a tree'),('f','requests: mark filled')),
     keys(('$','requests: top fillers'),('p','post')),
     head('reading'),
     keys(('left/right','previous/next'),('up/down','scroll')),
     keys(('r','comment (email: reply)'),('t','whole thread')),
     keys(('v','the thread as a tree'),('m','requests: mark filled')),
     keys(('g','files it links to'),('x','vote')),
     keys(('f','about the author'),('q','leave the boards')),
     head('votes'),
     keys(('x then +','vote up'),('x then -','vote down')),
     bottom()]
# The logon overview waits for a key itself; the guide is shown from the
# message menu (which pauses after it) and from the boards' h (which
# pauses when the screen doesn't).
news=len(sys.argv)>1 and sys.argv[1]=='news'
sys.stdout.buffer.write(('\x1b[0m\x1b[2J'+'\x1b[0m\r\n'.join(out)+'\x1b[0m\r\n').encode('cp437')+(b'|PA' if news else b''))
