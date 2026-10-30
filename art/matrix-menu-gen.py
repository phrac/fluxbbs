# menus/MATRIX.toml: the board's logon lightbar, themed to MATRIX.ANS
# (logon-gen.py). Run from art/: python3 -I matrix-menu-gen.py > ../menus/MATRIX.toml
items=[('log on','>L','','you have an account. prove it.'),
       ('apply','>A','','new here? we will decide if you stay.'),
       ('page sysop','>C','','ring the bell. someone may answer.'),
       ('feedback','EE','1;Matrix feedback;No','leave a note for the sysop.'),
       ('hang up','>H','','the smart choice. no hard feelings.')]
W=14  # each item, brackets included; MATRIX.ANS spaces the markers for it
out='''# The board's own MATRIX (the logon menu), in place of the stock one:
# the lightbar themed to MATRIX.ANS. Made by art/matrix-menu-gen.py.
type = "lightbar"
title = "Login Matrix"
prompt_name = "matrix"
help_file = "MATRIX"
prompt = ""
fallback = "MAIN"
generic_columns = 3
upper_input = true
'''
for k,(label,cmd,param,help) in enumerate(items,1):
    inner=label.center(W-4)
    off='|B0|08[ |07'+inner+' |08]'
    on='|B4|12[ |15'+inner+' |12]'
    out+=f'\n[[command]]\nkeys = "{k}"\ncommand = "{cmd}"\n'+(f'param = "{param}"\n' if param else '')
    out+=f'desc = "{label}"\nhelp = "|08> |07{help}"\non = "{on}"\noff = "{off}"\n'
print(out,end='')
