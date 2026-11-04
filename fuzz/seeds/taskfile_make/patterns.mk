%.o: %.c
	cc -c $<
$(OBJS): %.o: %.c
	cc $<
lib.a(m.o): m.o
build: CFLAGS += -O2
.DEFAULT_GOAL := $(firstword $(GOALS))
.SUFFIXES: .c .o
.c.o:
	cc -c $<
