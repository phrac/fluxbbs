; hello.com: INT 21h/09h, then exit with code 7.
        org 100h
        mov ah, 09h
        mov dx, msg
        int 21h
        mov ax, 4C07h
        int 21h
msg:    db 'Hello, DOS!', 13, 10, '$'
