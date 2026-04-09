.ifndef .L_MACRO_INC
.L_MACRO_INC:

.macro jlabel label
    glabel \label
.endm

.macro glabel label
    .global \label
    /* .n.b.! it seems is this necessary, but i don't see it in mwccgap */
    .type \label, %function
    \label:
.endm

.macro .def #
.endm

.macro dlabel label, local=0
    .ifeq \local
        .global \label
    .endif
    .type \label, @object
    \label:
.endm

.endif
