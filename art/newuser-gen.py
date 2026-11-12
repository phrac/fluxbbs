import os
import sys
# NEWUSER.ANS: shown to a caller still waiting to be validated, before
# they're told so and logged off (the logon's newuser step; NEWUSER
# menu). Iniquity's was its author's own board's (Eternity BBS). In the
# apply screen's purple. Run from text/:
#     python3 -I ../art/newuser-gen.py > NEWUSER.ANS
# A board's art/local.py can say it its own way: NEWUSER, a list of
# ('head', title), ('text', line) and ('', '') (a blank line); and
# NEWUSER_LOGO, the word over it and its TheDraw font (file, face).
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import boardname  # noqa: E402
NEWUSER = [('head', "you're in line"),
           ('text', 'Your account is waiting to be validated. Until it is, this is as far'),
           ('text', 'as it goes: nothing to do but call back.'),
           ('head', 'how you get in'),
           ('text', 'The [SysOp] looks new accounts over by hand. On some boards the callers'),
           ('text', 'vote new ones in, or confirming your [email] does it.'),
           ('head', 'while you wait'),
           ('text', "Call back in a day or so. If it's been longer, leave the SysOp [feedback]"),
           ('text', 'from the login screen.')]


def main():
    # The apply screen's pieces (its colors, rules and text lines),
    # without writing its screen.
    src = open(os.path.join(HERE, 'apply-gen.py')).read().split('\nAPPLY=[')[0]
    ap = {'__file__': os.path.join(HERE, 'apply-gen.py')}
    exec(compile(src, 'apply-gen.py', 'exec'), ap)
    items = boardname.local('NEWUSER', NEWUSER)
    word, file, face = boardname.local('NEWUSER_LOGO', ('welcome', 'BODYCNTX.TDF', 'Body Count P'))
    logo = ap['mm'].tdf_logo(file, face, word, 0).decode('cp437').rstrip('\n').split('\n')
    out = logo + ['', ap['rule']('new to ' + boardname.NAME.lower())]
    out += [ap['head'](t) if k == 'head' else ap['text'](t) if k == 'text' else '' for k, t in items] + [ap['bottom']()]
    sys.stdout.buffer.write(('\x1b[0m\x1b[2J' + '\x1b[0m\r\n'.join(out) + '\x1b[0m\r\n').encode('cp437'))


if __name__ == '__main__':
    main()
