.intel_syntax noprefix
# Extracted post-POPCNT/unrolling p12 bounded decoder, 64 symbols/iteration.
# Constants and initial states are established outside this measured loop.
# Error branches target an empty label; llvm-mca models straight-line execution.
# LLVM-MCA-BEGIN dc_p12_bounded64
.Lloop:
    vpcmpltud k1,zmm4,zmm6
    kmovw  eax,k1
    popcnt r14d,eax
    add    r14d,r14d
    mov    r15,rdx
    sub    r15,r9
    mov    al,0x6
    cmp    r15,r14
    jb .Lexit
    knotd  k2,k1
    vmovdqa64 zmm14,zmm5
    vpsrld zmm14{k2},zmm5,0x10
    vpsrld zmm4{k2},zmm4,0x10
    vpcmpnltud k0,zmm14,zmm4
    kortestw k0,k0
    jne .Lexit
    vpexpandw ymm15{k1}{z},YMMWORD PTR [rsi+r9*1]
    vpshufb ymm15,ymm15,ymm7
    vpmovzxwd zmm5{k1},ymm15
    vpsrld zmm15,zmm5,0x4
    vpandd zmm16,zmm15,zmm8
    vpxor  xmm15,xmm15,xmm15
    kxnorw k1,k0,k0
    vpgatherdd zmm15{k1},DWORD PTR [r11+zmm16*4]
    vpsrld zmm16,zmm15,0x18
    vmovdqu64 ZMMWORD PTR [rcx+r10*4],zmm16
    add    r14,r9
    vpcmpltud k1,zmm3,zmm6
    kmovw  r9d,k1
    popcnt r9d,r9d
    add    r9d,r9d
    mov    r15,rdx
    sub    r15,r14
    cmp    r15,r9
    jb .Lexit
    knotd  k2,k1
    vmovdqa64 zmm16,zmm13
    vpsrld zmm16{k2},zmm13,0x10
    vpsrld zmm3{k2},zmm3,0x10
    vpcmpnltud k0,zmm16,zmm3
    kortestw k0,k0
    jne .Lexit
    vpexpandw ymm17{k1}{z},YMMWORD PTR [rsi+r14*1]
    vpshufb ymm17,ymm17,ymm7
    vpmovzxwd zmm13{k1},ymm17
    vpsrld zmm17,zmm13,0x4
    vpandd zmm18,zmm17,zmm8
    vpxord xmm17,xmm17,xmm17
    kxnorw k1,k0,k0
    vpgatherdd zmm17{k1},DWORD PTR [r11+zmm18*4]
    vpsrld zmm18,zmm17,0x18
    vmovdqu64 ZMMWORD PTR [rcx+r10*4+0x40],zmm18
    add    r9,r14
    vpcmpltud k1,zmm2,zmm6
    kmovw  ebp,k1
    popcnt r14d,ebp
    add    r14d,r14d
    mov    r15,rdx
    sub    r15,r9
    cmp    r15,r14
    jb .Lexit
    knotd  k2,k1
    vmovdqa64 zmm18,zmm12
    vpsrld zmm18{k2},zmm12,0x10
    vpsrld zmm2{k2},zmm2,0x10
    vpcmpnltud k0,zmm18,zmm2
    kortestw k0,k0
    jne .Lexit
    vpexpandw ymm19{k1}{z},YMMWORD PTR [rsi+r9*1]
    vpshufb ymm19,ymm19,ymm7
    vpmovzxwd zmm12{k1},ymm19
    vpsrld zmm19,zmm12,0x4
    vpandd zmm20,zmm19,zmm8
    vpxord xmm19,xmm19,xmm19
    kxnorw k1,k0,k0
    vpgatherdd zmm19{k1},DWORD PTR [r11+zmm20*4]
    vpsrld zmm20,zmm19,0x18
    vmovdqu64 ZMMWORD PTR [rcx+r10*4+0x80],zmm20
    add    r14,r9
    vpcmpltud k1,zmm0,zmm6
    kmovw  r9d,k1
    popcnt r9d,r9d
    add    r9d,r9d
    mov    r15,rdx
    sub    r15,r14
    cmp    r15,r9
    jb .Lexit
    knotd  k2,k1
    vmovdqa64 zmm20,zmm1
    vpsrld zmm20{k2},zmm1,0x10
    vpsrld zmm0{k2},zmm0,0x10
    vpcmpnltud k0,zmm20,zmm0
    kortestw k0,k0
    mov    al,0x8
    jne .Lexit
    vpsrld zmm21,zmm15,0x8
    vpandd zmm21,zmm21,zmm9
    vpaddd zmm21,zmm21,zmm10
    vpslld zmm15,zmm15,0x4
    vpandd zmm5,zmm5,zmm11
    vpternlogd zmm5,zmm15,zmm9,0xf8
    vpmulld zmm14,zmm21,zmm14
    vpaddd zmm5,zmm5,zmm14
    vpmulld zmm4,zmm21,zmm4
    vpsrld zmm14,zmm17,0x8
    vpandd zmm14,zmm14,zmm9
    vpaddd zmm15,zmm14,zmm10
    vpslld zmm14,zmm17,0x4
    vpandd zmm14,zmm14,zmm9
    vpmulld zmm16,zmm15,zmm16
    vpaddd zmm14,zmm16,zmm14
    vpternlogd zmm14,zmm13,zmm11,0xf8
    vpmulld zmm3,zmm15,zmm3
    vpsrld zmm13,zmm19,0x8
    vpandd zmm13,zmm13,zmm9
    vpaddd zmm13,zmm13,zmm10
    vpslld zmm15,zmm19,0x4
    vpandd zmm15,zmm15,zmm9
    vpmulld zmm16,zmm13,zmm18
    vpaddd zmm15,zmm16,zmm15
    vpternlogd zmm15,zmm12,zmm11,0xf8
    vpmulld zmm2,zmm13,zmm2
    vpexpandw ymm12{k1}{z},YMMWORD PTR [rsi+r14*1]
    vpshufb ymm12,ymm12,ymm7
    vmovdqa64 zmm13,zmm1
    vpmovzxwd zmm13{k1},ymm12
    vpsrld zmm1,zmm13,0x4
    vpandd zmm1,zmm1,zmm8
    vpxor  xmm12,xmm12,xmm12
    kxnorw k1,k0,k0
    vpgatherdd zmm12{k1},DWORD PTR [r11+zmm1*4]
    vpsrld zmm1,zmm12,0x8
    vpandd zmm1,zmm1,zmm9
    vpaddd zmm16,zmm1,zmm10
    vpslld zmm1,zmm12,0x4
    vpandd zmm1,zmm1,zmm9
    vpsrld zmm12,zmm12,0x18
    vpmulld zmm17,zmm16,zmm20
    vpaddd zmm1,zmm17,zmm1
    vpternlogd zmm1,zmm13,zmm11,0xf8
    vpmulld zmm0,zmm16,zmm0
    vmovdqu64 ZMMWORD PTR [rcx+r10*4+0xc0],zmm12
    add    r9,r14
    add    r10,0x40
    add    rbx,0xffffffffffffffc0
    vmovdqa64 zmm12,zmm15
    vmovdqa64 zmm13,zmm14
    cmp    rbx,0x3f
    ja .Lloop
.Lexit:
# LLVM-MCA-END dc_p12_bounded64
