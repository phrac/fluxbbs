; files.com: create OUT.TXT, write, close, reopen, read it back and
; print it, find it, delete it. Exits 0 on success, else the step.
        org 100h
        mov ah, 3Ch
        xor cx, cx
        mov dx, name
        int 21h
        jc fail1
        mov bx, ax
        mov ah, 40h
        mov cx, 6
        mov dx, text
        int 21h
        jc fail2
        mov ah, 3Eh
        int 21h
        mov ax, 3D00h
        mov dx, name
        int 21h
        jc fail3
        mov bx, ax
        mov ah, 3Fh
        mov cx, 32
        mov dx, buf
        int 21h
        jc fail4
        cmp ax, 6
        jne fail4
        mov ah, 3Eh
        int 21h
        mov ah, 40h
        mov bx, 1
        mov cx, 6
        mov dx, buf
        int 21h
        mov ah, 4Eh
        xor cx, cx
        mov dx, wild
        int 21h
        jc fail5
        mov ah, 41h
        mov dx, name
        int 21h
        jc fail6
        mov ax, 3D00h
        mov dx, name
        int 21h
        jnc fail7
        mov ax, 4C00h
        int 21h
fail1:  mov ax, 4C01h
        int 21h
fail2:  mov ax, 4C02h
        int 21h
fail3:  mov ax, 4C03h
        int 21h
fail4:  mov ax, 4C04h
        int 21h
fail5:  mov ax, 4C05h
        int 21h
fail6:  mov ax, 4C06h
        int 21h
fail7:  mov ax, 4C07h
        int 21h
name:   db 'out.txt', 0
wild:   db '*.TXT', 0
text:   db 'Hi', 13, 10, 'ok'
buf:    times 32 db 0
