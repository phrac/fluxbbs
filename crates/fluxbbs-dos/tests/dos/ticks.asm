; ticks.com: wait two timer ticks via INT 1Ah, then wait for a key
; through INT 16h and print it with INT 10h/0Eh.
        org 100h
        mov ah, 00h
        int 1Ah
        mov bx, dx
        add bx, 2
.wait:  mov ah, 00h
        int 1Ah
        cmp dx, bx
        jb .wait
        mov ah, 00h
        int 16h
        mov ah, 0Eh
        int 10h
        mov ax, 4C00h
        int 21h
