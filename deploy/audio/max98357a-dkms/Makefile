obj-m += snd-soc-max98357a.o
snd-soc-max98357a-objs := max98357a.o

KVER ?= $(shell uname -r)

all:
	make -C /lib/modules/$(KVER)/build M=$(PWD) modules

clean:
	make -C /lib/modules/$(KVER)/build M=$(PWD) clean
