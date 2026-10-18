; echo.com: a FOSSIL door. Initialise, send a banner, echo what
; arrives (upper-cased) until 'q', then lower DTR and exit.
        org 100h
        mov ah, 04h
        xor dx, dx
        int 14h
        cmp ax, 1954h
        jne nofossil
        mov si, banner
.send:  lodsb
        or al, al
        jz .loop
        mov ah, 01h
        int 14h
        jmp .send
.loop:  mov ah, 02h
        xor dx, dx
        int 14h
        cmp al, 'q'
        je .done
        cmp al, 'a'
        jb .out
        cmp al, 'z'
        ja .out
        sub al, 20h
.out:   mov ah, 01h
        int 14h
        jmp .loop
.done:  mov ah, 06h
        xor al, al
        int 14h
        mov ax, 4C00h
        int 21h
nofossil:
        mov ax, 4C01h
        int 21h
banner: db 'READY', 13, 10, 0
